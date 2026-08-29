# Changelog

All notable changes to Nexus (formerly Tempo) are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.9.2] — 2026-08-29

### Added

- **Amplifier controls in every cockpit.** With an amplifier configured — SPE Expert or Elecraft
  KPA500/KPA1500 — each operating screen's header carries a compact strip: Standby/Operate, band
  down and up, and power out, so you can put the amp in line or move it a band without leaving
  the screen you are working on. It appears only when an amplifier is set up, and nothing at all
  is added for the stations that have none. Operate reads from the amplifier itself rather than
  from your click, so the button always shows where the amplifier actually is, even if you press
  the front-panel key instead. Both controls are refused while you are transmitting: changing
  band on a keyed amplifier can damage it, and dropping to standby mid-over does not stop
  anything — the exciter keeps keying and the drive passes straight through. Nexus will never
  switch your amplifier off; that command does not exist in the code. Asked for by KD9TAW.

  ⚠️ The Elecraft side has never been run on hardware — it is written from Elecraft's own
  programmer's references and is waiting on someone with a KPA to try it. The SPE side has been
  tested against a 1.5K-FA.

- **Nexus can read your amplifier.** Put an SPE Expert (1.3K-FA, 1.5K-FA, 2K-FA) or an Elecraft
  KPA500/KPA1500 on its own serial port, set it under **Settings ▸ Radio ▸ Amplifier**, and place
  the **Amplifier** pane in Connect: power out, SWR at the antenna and before the tuner, supply
  volts and current, PA temperature, and the amplifier's own alarms and warnings. It only ever
  asks — the one thing Nexus can send this amplifier is the six-byte status request, so it cannot
  switch it off, put it in standby, change band or start a tune. Readings clear the moment a poll
  goes unanswered, because a stale wattage beside a dead link is worse than no wattage; an alarm
  code this build has never seen still shows as a fault rather than going quiet; and the port is
  checked against everything else on the station, since a serial port opens only once and an
  amplifier typed onto the CAT port takes the *radio* down. Verified against a real 1.5K-FA. The
  Elecraft side is written from Elecraft's published references and has not been run on hardware.

- **A zero-beat light in the CW cockpit.** The scope has always drawn a marker at your CW
  pitch; now the app measures the tone actually coming in and tells you where it sits against
  it. A light comes on when you are on pitch, and beside it a needle and a signed offset in Hz
  say which way and how far off you are — being 80 Hz out no longer looks the same as being
  400 Hz out. How close counts as on pitch follows your rig's CW filter (25 Hz behind the usual
  500 Hz one, tighter behind a narrow filter). When several signals are in the passband it
  follows the one nearest your marker rather than the loudest, so it does not jump to a strong
  station 300 Hz away while you are closing in. On a dead band it goes quiet and stays quiet: a
  reading only appears once a tone has held the same frequency long enough to be a signal you
  are tuning rather than the loudest thing the receiver happened to hear, so the needle does not
  twitch at band noise between overs. It waits about a fifth of a second before showing a new
  signal, which is the price of that stillness, and it keeps tracking live once it has one — the
  wait is paid when a station starts sending, never between their letters. It is a display only;
  it never touches your dial. Asked for by KD9TAW.

- **Tune can key at its own power.** Set a tune power under **Settings ▸ Digital ▸ Transmit &
  Sequencing** and a tune-up keys at that level instead of whatever you are running. It can only
  ever turn the rig *down* — it keys at whichever is lower, your setting or your current power —
  so it can never lift you past a per-mode limit. Left empty it does nothing at all, which is the
  default. Asked for by an operator running an MFJ-1786 loop, where a hundred watts into an
  untuned loop is not what you want while the tuner hunts.

- **Split contacts log both frequencies.** A contact worked split now records the frequency you
  transmitted on and the frequency you received on, as separate ADIF fields, so a logger you export
  to sees the same pair you actually used. Both values were already on screen and were being thrown
  away at the log boundary. Not split? Nothing changes and no second frequency is written — an
  invented one would be a false claim of split that every logger downstream believes. Satellite
  passes are deliberately unchanged for now; doing them properly means moving the band field too,
  which reaches every award and needed-list in Nexus. Asked for by swann.

- **PSK has a log strip.** The PSK cockpit now carries the same log strip as CW and Phone, PSK31
  and QPSK31 are in the mode list, and the manual log form stops pre-filling FT8 for a PSK
  contact. It logs the sub-mode you actually worked, so a QPSK31 QSO records QPSK31. Reported by
  KR4FQG.

- **A callsign card in the FT8 cockpit.** Clicking a call in the roster now shows what you already
  know about that station — previous contacts with band, mode and date, what is unconfirmed, the
  new-one flags, distance and bearing, and your private note — the same card CW and Phone have
  had. It follows the station you are working as well as one you click, so it is populated through
  a normal run rather than only when you go looking. Reported by KR4FQG.

- **RTTY starts receiving when you open it.** Every other decode mode auto-arms; RTTY was the one
  that waited for you to press Arm RX, which is a likely reason behind "RTTY is not decoding".
  Same Settings opt-out as PSK, APRS and SSTV.

- **Tell Nexus what your serial cable actually does.** Two new declarations under **Settings ▸
  Radio ▸ Rig & CAT ▸ Advanced** — serial handshake, and the keying line's state at startup. Both
  default to **Auto**, which is exactly today's behaviour, so nothing about your station changes
  by upgrading. They exist for one fault: a rig that keys the transmitter the moment Nexus opens.
  If that is not happening to you, leave them alone.

- **The app sizes itself to a high-resolution screen on Linux.** On a 4K panel, or a small sharp
  laptop screen, Nexus used to open at 1:1 with the screen's pixels and read half the size the same
  app does on Windows — Windows and macOS tell an app how dense the screen is, and Linux does not.
  Nexus now asks the display for its physical size on first launch and lifts its own zoom ceiling to
  match, so the interface can grow to the screen instead of being pinned at 100%. It only ever makes
  things bigger, only on a screen that genuinely needs it, and only once: set the zoom yourself under
  **Settings ▸ Appearance** and your choice stands for good. On an ordinary monitor, and on Windows
  and macOS, nothing changes at all. Reported by an operator running 1920×1080 on a twelve-inch panel.

### Fixed

- **Soundcard CW keying now uses a data mode, so the audio actually reaches the transmitter.**
  With the CW keyer set to Soundcard, Nexus put the radio into plain USB or LSB. On the common
  Icom and default Yaesu wiring, plain SSB takes its transmit audio from the MIC socket, so the
  rig keyed and radiated nothing — the red light came on and no signal went out. It now commands
  the DATA submode (DATA-U/DATA-L), which is what FT8, PSK31, RTTY and SSTV have always done;
  soundcard CW was the one path that did not. ⚠️ **If your interface feeds the rig's MIC socket
  and soundcard CW was working for you, tick "plain SSB for data modes" on that radio in Settings
  and it will work again** — that is the same switch the other four modes already use. Reported by
  an FTX-1 operator.

- **The CW cockpit and Settings now warn when CAT keying is unproven on your radio.** On the
  Yaesu FTX-1, the keying command Hamlib sends differs from the one it uses on other radios, and
  it reports success either way — so if nothing goes out, Nexus has no way to know and the keyer
  error line stays dark. Rather than let you lose an evening to it, both the Settings picker and
  the cockpit's live keyer switch now say it is unproven on that radio and point you at the
  serial keyline, WinKeyer or the soundcard keyer. It is a notice, not a block: nothing stops you
  using CAT keying, and nothing about what Nexus sends has changed.

- **Switching to the Soundcard keyer says that it takes the radio out of CW.** It always did —
  a keyed audio tone needs the rig in an SSB or data mode — but nothing on screen said so, so the
  mode change looked like a bug. The Settings hint and the cockpit's keyer switch now both say it,
  and say that CW mode comes back when you pick another keyer.

- **The Work button left the markers behind.** Clicking Work on a station card in the Classic
  layout's Stations pane started the QSO but did not move your RX and TX markers onto the
  station, so you answered someone while still listening and transmitting on a different
  offset. Band Activity's double-click and the Roster table both moved them; the card did not,
  which made one gesture behave three ways in the same cockpit. Both marks now follow the
  station, RX always and TX unless you have Hold Tx Freq set, the same as WSJT-X. Reported by
  bitslave.

- **eQSL sync blamed your password when the password was fine.** If the download came back as
  anything other than the log file, Nexus told you to check your username and password — at a
  point where the login had already succeeded. Uploads kept working, because they never go
  through that check, so the app was simultaneously proving your credentials were right and
  telling you they were wrong. It now says what actually happened and states plainly that it is
  not a login problem.

  The LoTW sync carried the identical fault and has been corrected the same way, before anyone
  hit it — that is the one that matters for award credit.

- **The SWR and ALC meters had no warning band.** The bar that should turn amber as SWR climbs
  was painting the same red as the "too hot" band, so there was nothing between "fine" and
  "trouble" — you saw a problem arriving rather than coming. Same on the ALC and the S-meter.
  The colour was written correctly and never took effect. Green, amber, red now, as intended.

- **A paused waterfall looked like an error.** Same cause as the meters: the pause indicator was
  reaching for an amber that never applied, and painted the error red instead.

- **Small print was rendering at full size in twenty places.** Text written to be small — panel
  tags, beacon and band rows, the CW decoder's status and age lines, the waterfall pop-out
  controls, APRS beacon titles — was silently falling back to body size, so it sat at 14px beside
  neighbours at 12px and 11px. Now the size it was always meant to be.

- **Colours that could not follow the theme now do.** Around eighty surfaces were painting
  hard-coded colours rather than the app's own palette: reds that did not match each other (three
  different ones), greens likewise, and ambers that stayed identical whether you were in light or
  dark. Worst of it was on the light theme, where several recessed areas — the connections log,
  the waterfall pop-out, the SSTV picture area, the CW decode strip — rendered as grey slabs
  because their colour was a fixed black wash chosen against a dark panel. The settings search
  results list had no background at all and drew straight over whatever was behind it.

- **eQSL sync refused a perfectly good download.** eQSL stopped starting its InBox export with
  the words Nexus was looking for, so the sync rejected the file and told you your credentials
  were wrong — at a point where the login had already succeeded. Nexus now checks the markers
  that identify an eQSL export rather than a sentence eQSL can reword whenever it likes. An HTML
  error page is still refused, which is what that check was for in the first place. Found,
  diagnosed and fixed by KR8MER.

- **Dropdowns were white on white on Linux.** Every dropdown in the app drew as a white box with
  the app's own pale text on it, which on a dark theme meant you could not read what was selected
  — including the radio and sound-card pickers in step 2 of first-time setup, so a new operator
  could not see what they were choosing while setting the app up. Linux draws form controls with
  its own widget theme unless an app takes them over, and Nexus never did; Windows and macOS were
  never affected, which is why the same build looked fine there. Every dropdown is now drawn by
  Nexus itself, in your theme, with its own arrow. Reported by an operator on Ubuntu 24.04 with an
  FT-991A, and by M0LHJ on Fedora. **The open list is a separate problem** and is not fixed here:
  Linux draws that as a system menu that an application cannot style at all, so the list you see
  after clicking may still use your desktop's colours.

- **Grey slabs in the light theme.** On the light theme, the CW decode strip, the copilot strip,
  and the SSTV image area, drop zone and progress bar all drew as flat mid-grey blocks — the SSTV
  picture area worst of all, since it is the largest. Each was asking the stylesheet for a colour
  that had never been defined, and falling back to a fixed dark wash that happens to look right on
  the dark theme and muddy on the light one. They now use the same recessed colour the rest of the
  app uses, which is defined for both themes. The dark theme is unchanged.

- **Four Settings dropdowns were the wrong size.** The rotator model, pounce threshold, WAV
  recording and propagation engine pickers rendered noticeably shorter than every other control on
  the same page. They now match.

- **A guessed radio model no longer looks confirmed.** When auto-detect proves the port but has to
  guess the model — an FT-991A answers a probe meant for an FTDX10 — the wizard marks the model box
  for you to confirm. That marking had never actually appeared on screen, on any platform, so the
  one moment you could catch a wrong radio passed by silently. The box is now outlined.

- **APRS reported packets that were never there.** With the squelch open, the APRS panel counted
  plain noise as packets failing their checksum — about one every four seconds — so within moments
  of listening you were told the channel was full of traffic Nexus could not read. Nothing was
  wrong with the decoder; the counter simply accepted any burst of noise that happened to look
  frame-shaped. It now checks that what it found could actually be an AX.25 frame before counting
  it. A real packet that genuinely fails its checksum is still counted, which is what that reading
  is for. APRS also writes to the diagnostic log now — it was the only mode that said nothing at
  all, so an APRS problem arrived with a log that talked exclusively about FT8. Reported by swinn.

- **"Check your network" was the answer to problems that had nothing to do with your network.**
  Every connector — QRZ, ClubLog, LoTW, eQSL, HRDLog and the rest — reported a rejected secure
  connection as a network failure. So if antivirus or a company proxy inspects your HTTPS traffic,
  Nexus would fail to upload while your browser worked perfectly and every check you could run said
  your connection was fine. It now tells the two apart and says when interception is the likely
  cause. A genuinely unreachable network still says so. Raised by lz2aov.

- **Test CAT told you to close other software when the real problem was permissions.** On Linux,
  a serial port your user is not permitted to open reports "permission denied" — and Nexus
  answered with the advice for a *busy* port, telling you to close WSJT-X and flrig. No amount of
  closing programs grants a group membership, so you would close everything, test again, fail
  again, and reasonably conclude Nexus does not do CAT on Linux. It now recognises a permission
  refusal and gives the actual cure: add yourself to the `dialout` group, then log out and back in
  — the second half matters, because a group does not apply to a session already running. A port
  another program really is holding still gets the advice it always did. Reported from Ubuntu
  24.04 with an FT-991A that worked on Windows on the same machine.

- **Audio broke up badly on Windows, and worse on battery.** The sound-card callback and the code
  feeding it were fighting over the same lock, and the feeding side held it while copying a whole
  transmission — so when Windows slowed the cores down on battery, the callback missed its deadline
  and put silence on the air. That is why raising Nexus's priority helped, and why WSJT-X on the
  same laptop was clean. The two no longer share a lock. There are underrun counters behind it now,
  so the next report comes with a number instead of an impression. Reported by KR8MER.

- **No decodes, and a log that grew to 294 MB.** Nexus opened the playback side of your sound card
  at startup whether or not you ever transmitted. On a codec that cannot record and play at the same
  time, that open wedges, and the error retried forever — millions of identical lines — while
  capture kept working, so the waterfall looked healthy and nothing ever decoded. Nexus now opens
  the playback side when you first transmit, so a station that is only listening opens it at all.
  Repeated errors are also capped: 22 lines with a count on each, instead of filling your disk.
  Reported by M0LHJ, who did the diagnostic work that found it.

- **Your radio's audio card could be missing from the transmit list on macOS.** A card that only
  plays and does not record was dropped from the list entirely, so the rig keyed with no audio and
  nothing on screen explained why. Two operators hit it on different interfaces. Reported by
  pvanderp and crabtreejw; the cause was found by M7HNF-Ian, whose diagnosis this fix follows.

- **Some sound cards were refused outright, with no audio at all.** Nexus understood four of the ten
  audio formats a card can report. A card whose natural format was any of the other six got an
  "unsupported format" message and silence — not a setting you could change, and nothing you did
  wrong. All formats are carried now. If you have an interface that never worked with Nexus and you
  could never find out why, it is worth another try.

- **Nexus no longer adopts a rigctld that is driving a different radio.** When a rigctld was already
  listening on a radio's CAT port, Nexus connected through it — which is the right thing, and is how
  it shares a rig with WSJT-X — but it never checked WHICH radio that daemon was attached to. On a
  two-radio station a stray daemon left over from the other rig would be adopted, and from then on
  every frequency read, every band change and **every keying command went to the wrong radio**, with
  the app showing the other rig's dial as though it were yours. Nexus now asks first: it reads the
  daemon's own arguments (which carry the model AND the serial device) and falls back to asking the
  daemon over the protocol when there is no local process to read. On a mismatch it refuses, says
  which radio that daemon is actually driving, and tells you how to fix it — stop the daemon, or
  give this radio its own rigctld port. Sharing a daemon that IS this radio's is unchanged, and so
  is coexisting with an external NET-rigctl station.

- **Tune could drop out repeatedly, defeating an automatic antenna tuner.** On a station with an
  auto-tuning antenna — a magnetic loop especially — the tune carrier was interrupted over and
  over. A tuner reads those gaps as a match, beeps, and stops before it has found one. Reported by
  an operator running an MFJ-1786 with an IC-7300.

  Two causes, both ours. Nexus queued only about 40 ms of tune audio ahead of the sound card while
  asking the radio for meter readings on the same thread — and a radio that is slow to answer
  stalls that thread for longer than the audio it has queued, so the carrier stops while the rig
  stays keyed. Separately, if you had a per-mode power limit set, Nexus re-sent that limit to the
  radio *every twenty milliseconds* for the whole tune. The meters now stand down during a
  tune-up, the power limit is not re-sent under a live carrier, and the carrier runs a quarter of
  a second ahead instead of forty milliseconds.

- **A rig could key the transmitter the moment Nexus started.** With PTT set to serial RTS and no
  separate PTT port, Nexus told the rig-control daemon nothing about the RTS line at all, so its
  idle state fell to whatever the driver happened to do — on some cables, keyed. Two earlier
  attempts guessed at this from other settings. Rather than guess a third time, the handshake and
  the keying line's startup state are now things you can state outright (see Added). Reported by
  VK6MO.

- **Sharing your radio could report success for a mode it never set.** If another program —
  VarAC, FreeDV, WSJT-X, a logger — asked Nexus for a data mode while Nexus was in a voice
  section, Nexus answered "done" and left the rig in plain SSB. Your data signal went out on a
  voice emission with nothing on screen to say so. It now answers honestly, and the connection log
  names the mode that was asked for and the mode your radio is actually in. Reading the mode back
  is honest too, on an FM calling channel, an APRS park or during an SSTV picture. Reported by
  rogerloxton. **This is a behaviour change you may notice — see Changed.**

- **A refused PTT from another program said nothing at all.** Nexus starts with transmit disarmed,
  so the first time an external program tried to key, it was refused silently — which is why
  pressing PTT in Phone appeared to "wake it up". The refusal now says why, in the connection log.

- **Sharing could fill your connection log at one line a second, forever.** If something else
  already owned the sharing port — your own rigctld, a second copy of Nexus — Nexus retried the
  bind every second and logged every failure, without ever telling you plainly. It now says so
  once, names the port and the likely cause, and backs off.

- **Callsigns could be given the wrong country and a nonsense state.** WL7E showed as Alaska with
  a state of "CA". The country comes from the callsign prefix and the state came from the FCC
  index, which is a mailing address — and nothing compared the two. An Alaskan or Hawaiian call
  now gets AK or HI, and a station outside the US gets no state at all. This also fed the Worked
  All States "new one" cue, so it was quietly wrong there too. Reported by KR4FQG.

- **Settings you changed while editing another radio were thrown away.** If you clicked Edit on a
  radio you were not currently using and then changed anything station-wide — QRZ auto-upload, for
  instance — the panel said it saved and nothing was written. This seam has now cost a keying
  port, three Flex fields, an OmniRig slot and an Icom submode; the save itself is fixed rather
  than another field name being added to it. Reported by barnburner6503.

- **Your own transmissions vanished from the Rx Frequency pane.** Stopping transmit, changing band
  or switching radio wiped the record of overs you had already sent — including when Nexus did it
  for you, with nobody touching anything. Reported by Luk73.

- **"Calling you" alerts only fired after the contact had ended.** An alert for the station you
  were working was suppressed — and the station answering your CQ becomes that station in the same
  instant the decode arrives, so the alert you actually wanted was the one that got suppressed.
  Your own callsign now always raises the alert, as it does in WSJT-X. Reported by KR4FQG.

- **LoTW confirmations were recorded as paper QSL cards.** A LoTW report carries an ordinary
  confirmation field, and Nexus read it as a card — so the log claimed a card you never received,
  and an ADIF export carried that claim on to any other logger. A hand-imported third-party ADIF
  still counts as a card, correctly. Award credit is unaffected either way. Reported by rgoiko.

- **A QSL-sent mark could not be undone.** Mis-click the QSL menu and it was permanent. There is a
  clear entry now, and the clear sticks even if you later re-import a log that still says the card
  went out. Reported by rgoiko.

- **WSPR could beacon where nobody was listening.** The transmit marker could be dragged anywhere
  from 200 Hz to 4 kHz, while every WSPR decoder searches 1400-1600 Hz. You could beacon all night
  and be heard by nobody. It is held inside the sub-band now, matching WSJT-X. FT8, FT4 and FST4W
  are unchanged. Reported by akhepcat.

- **A repeated report went unanswered, and a station returning late got silence.** When a contact
  closed, Nexus dropped the station immediately — so a partner who repeated their report got
  nothing back, and a station the run had given up on that came back with RR73 never got a closing
  73. WSJT-X keeps the station and answers both. Nexus now does too, bounded to a single over
  within three minutes, without re-entering the contact. Reported by KR4FQG and bitslave.

- **The update notice did nothing on Linux, and silenced itself.** Clicking Download dismissed the
  pop-up and opened nothing — and it recorded the version as dismissed anyway, so the notice never
  came back. Anyone who clicked it once stopped being told about updates entirely. It now opens
  the page or tells you it could not and gives you the link to copy.

- **A radio's audio could be refused by the monitor while working everywhere else.** On Linux the
  headphone monitor picked the first audio format the card advertised rather than the one it could
  actually use, and reported "unsupported format" for a card the rest of Nexus was using happily.
  Reported by MW0CQU.

- **Satellite elements stopped updating.** The upstream catalogue renamed the status that marks a
  satellite as being in orbit, and the mirror correctly refused to publish rather than ship an
  empty list — which meant satellite data quietly went stale instead. Fixed, and the check now
  refuses an unknown status by name rather than only noticing when everything moves at once.

- **Icom DATA submode was quietly reset whenever you edited a radio you were not using.** If you
  run more than one radio and set an Icom to DATA2 or DATA3, opening that radio's entry and
  saving it put it back to DATA1 without saying so. The panel reported success, the setting was
  gone, and the next time you keyed that rig it was in the wrong submode.

  The per-radio Edit form saves through a patch that the app and the radio settings each
  describe separately, and the submode was missing from one of the two descriptions. Anything
  missing there is filled in with a default rather than your value. It travels with the patch
  now, and there is a check that fails the build if the two descriptions ever disagree again —
  the same drift has cost a keying port, a Flex address and an OmniRig slot before this.

### Changed

- **A station that is only receiving no longer opens your sound card's playback side at all.** You
  will see this if you look — the playback stream simply is not there until the first time you
  transmit. That is the fix for the wedged-open problem above, and it also means one fewer thing
  holding your card if you share it with another program.

- **Your sound card may open at a different rate or format after this update.** The audio library
  Nexus uses now picks the card's default differently — 48 kHz where it might previously have chosen
  44.1 kHz, and a different sample format on some cards. Nexus adapts either way and you should not
  hear a difference, but if you keep notes on your station's audio settings, this is the release
  where a number may move on its own.

- **Backup and Restore moved to their own Config tab.** They existed, but sat under **Radio ▸
  Transmit limits & sharing**, which is why operators did not find them: backing up a whole
  station has nothing to do with transmit limits. Same controls, same behaviour — a findable
  home, and search keywords wide enough to survive a panic ("backup", "restore", "factory",
  "defaults", "start over").

- **Sharing your radio now refuses a mode it cannot set, where it used to accept it silently.**
  This is the honest half of the fix above, and it is the one you might feel: a program configured
  for a data mode while Nexus sits in Phone now gets a real error instead of a quiet success. Put
  Nexus in the section that matches what the other program is doing — Digital for FT8 and
  VarAC-style data, Phone for voice, CW for CW — and it goes through exactly as before. If you do
  see a refusal, the connection log names both modes so you can see which end to change.

- **The spots filter says what it does.** "Heard near me" is now "Heard on my continent", which is
  what the filter has always actually done. Asked about by barnburner6503, who went looking for a
  Europe-only filter that was already there and switched on.

- **Grids by band lists the VUCC bands by default.** Grids count toward an award on 50 MHz and
  up and nowhere else, so on an HF station the list was mostly grids that count toward nothing,
  with the few bands that do count buried in it. It now starts at 6 m and up, with an **All
  bands** button if you want the full count back.

  The VUCC box above it has always been VHF-only and is unchanged. Suggested by NT9E.

- **macOS: the Settings pickers can now tell two identical radios apart.** On a station with
  two rigs that use the same bridge and codec chips, every serial port carried the same product
  label ("CP2105 Dual USB to UART Bridge Controller", eight times) and both sound cards
  enumerated as "USB Audio Device" — with only a positional " #2" between them, assigned by
  enumeration order. Moving one rig to a different USB socket therefore swapped which radio each
  saved name referred to, silently, with nothing to warn you. Nexus now reads USB topology on
  macOS and uses it three ways. **Rig auto-detect and the CAT port probe** no longer see the same
  physical port twice: a Silicon Labs bridge is offered once by Apple's driver
  (`cu.usbserial-…`) and again by the vendor's (`cu.SLAB_USBtoUART…`), two names no rule could
  pair, so Detect listed each rig twice and the baud sweep spent a full ladder probing a port it
  had already tried. **Saving now warns** if you picked the half of a dual bridge that carries no
  CAT — the most convincing way to make a working radio look dead, because the port opens and the
  writes succeed and nothing ever answers. **And saving warns** if the sound card you chose is
  inside the *other* radio. Both warnings are advisory and never block a save: an
  unusual-but-correct station is still yours to configure. The port list itself is unchanged —
  every port you could pick before, you can still pick.
## [1.9.1] — 2026-08-26

### Added

- **CW and Operate have a real split control.** They only ever *displayed* that split was on.
  Phone has had a proper one for a while; now all three do, so you can set up a split from the
  cockpit you are working in rather than reaching for the radio.

- **Nexus can follow the radio's own split**, if you turn it on in Settings and your radio can
  report it reliably. Nexus asks the radio rather than asking you to guess — on a radio that
  cannot answer without being disturbed, the option is not offered, because finding out would
  mean moving your VFOs behind your back. Off by default.

### Fixed

- **RTTY: Nexus could let you transmit outside your licence privileges, and could ignore your
  Reverse setting.** Both arrived in 1.9.0 and both are on the transmit path, so this is worth
  taking even if neither has bitten you.

  1.9.0 made RTTY transmit on the frequency you tuned to, which was the right fix — but the
  privilege check was still working from the old fixed tone and never learned about it. Click
  the waterfall to net onto a station and your signal moves; the check did not. It could be out
  by as much as 1.9 kHz, and near the bottom of a band segment that is the difference between
  legal and not. It now works from the tone you are actually sending.

  Separately, **Reverse did nothing on transmit.** It was being applied twice and the two
  cancelled out, so a rig set up for reversed tones decoded fine and answered the wrong way
  round — the far end saw nothing. It applies once now, and receive and transmit agree.

- **Working split no longer locks you out of transmitting, and no longer lets you transmit where
  you should not.** The licence check judged your *receive* dial. Under split those are two
  different frequencies, and it was wrong in both directions.

  Receiving on a DX station in a segment you may not transmit in — the everyday way DX is
  worked, since expeditions sit in the quiet part of the band and listen up where the pile-up
  can answer — got you a TX lock even though your transmit frequency was perfectly legal. And
  the reverse: a legal receive frequency with the transmit VFO parked somewhere you may not use
  would key without complaint.

  Nexus now judges the frequency your signal actually leaves on. When it cannot tell where that
  is, it refuses rather than guessing.

## [1.9.0] — 2026-08-25

### Added

- **CAT rig control now works out of the box on Linux and macOS.** Nexus carries Hamlib —
  the `rigctld` program it drives your radio through — inside every download, on every
  platform. Nothing to install, no Homebrew, no apt.

  Windows has worked this way since the beginning. Linux and macOS did not, and the AppImage
  was the worst of it: an AppImage is the download you choose *because* it installs nothing,
  and it was the one that could not talk to a radio until you worked out on your own that you
  needed a package called `libhamlib-utils` that nothing had ever mentioned. It shipped
  Hamlib's licence files and no Hamlib. macOS had the same hole behind `brew install hamlib`.

  If you already installed Hamlib yourself, nothing changes and nothing conflicts — Nexus
  prefers its own copy, and falls back to yours if that ever fails to start.

- **Nexus speaks Spanish and French.** Both are picked up automatically from your system
  language, or you can choose one in Settings. The whole application is translated — all 4,681
  phrases, every screen, the same coverage German has. Anything a future release adds shows its
  English wording until it is translated, rather than a blank.

  Frequencies, callsigns, grid squares, signal reports, mode names and every other on-air term
  stay exactly as they are in all languages. That is deliberate: Spanish and French both write a
  decimal comma, and a dial reading 14,074 is an operating fault, not a cosmetic one.

### Fixed

- **The SSTV waterfall comes back.** Change band while a picture was coming in and the band
  display stopped and stayed stopped — switching modes and back was the only way to get it
  returned, and landing on an SSTV frequency stopped it again.

  The screen shows the band until a picture starts arriving and then shows the picture in the
  same place. A decode that began and never finished was never cleaned up, so the app went on
  believing a picture was still coming and held the display for it — for the rest of the
  session. Changing band is the obvious way to cause that, but so is the sending station
  stopping mid-picture, or the band simply going long.

  A picture that has run well past the time its own mode takes is now given up on and the
  waterfall returns. Nothing is given up early: the allowance is per mode, so a Scottie DX gets
  its four and a half minutes.


- **Picking a RTTY frequency from the band plan now puts your signal where the plan says.** The
  listed frequencies were chosen as the frequency your signal comes out on — which is what the
  dial reads on true FSK, but not on AFSK, the default. On AFSK the tones sit about 2.3 kHz below
  the dial, so the signal landed low: on 20 m it was inside the FT4 cluster, on 17 m and 12 m
  inside FT8, and on 15 m inside JS8 — the exact overlaps the band plan was written to avoid.
  Both keying backends now land in the same place, and it is the place the plan describes. Your
  dial reading will look about 2.3 kHz higher than before on AFSK; the signal is what moved back
  where it belongs.

- **A zoomed waterfall stays where you put it.** Picking a numeric span made the display
  re-centre on your receive marker every time you clicked, so each click slid the view sideways
  by up to half a span with nothing to scroll it back. The zoom is a slice of the passband now:
  it holds still while your marker is in view, and only moves when you tune outside it — so it
  still cannot end up showing you the wrong part of the band. The default Std view was never
  affected.

- **Linux: Nexus stops asking the system keyring whether it could upload when it has nothing to
  upload.** The auto-upload worker checked your ClubLog credentials every two seconds whether or
  not a single contact was waiting, and on Linux each check is a round trip to the keyring
  daemon. That is the same thing that was restarting gnome-keyring in a loop before 1.8.0, at
  more than twice the rate, in a different place. Windows and macOS were never affected — the
  check there is a local call.

- **Recording a QSL card that arrived no longer depends on a filter — or on not having sent
  one.** The control shipped in 1.8.0 could not be reached: it only appeared while the "needs
  confirmation" chip was on, so it was invisible in the ordinary Logbook where you work through a
  stack of cards. Worse, marking a card *sent* removed the menu, so the card that came back
  months later could never be recorded — the whole point of the feature. It is on every row now,
  and you still cannot mark one sent twice.

- **Contacts sent to Log4OM (and anything else on the N1MM broadcast) carry both signal
  reports.** Sent and received RST were missing from every QSO — the sent one only travelled in a
  field loggers read as contest exchange data, and the received one was not sent at all. The same
  bug in the N3FJP broadcast was fixed a while back and this one was missed with it.

- **RTTY transmits on the frequency you tuned to.** Clicking the waterfall to net onto a station
  moved the decoder but not the transmitter, which stayed on the default tone pair — so you
  answered on a frequency nobody was listening on. Your dial does not move; only the audio
  offset, exactly like the FT8 transmit marker.

- **The Tempo roster works on Tempo Deep.** It only ever listed stations heard on Tempo Fast, so
  on Deep it was always empty — which reads like a quiet band rather than a fault.

- **The compact Band Activity in the Tempo rail shows everything again.** It hid the filter chips
  but still applied whichever one you last chose in Operate, so a "CQ" chip set elsewhere quietly
  filtered the list with nothing on screen to explain it.

- **A directed CQ (`CQ DX`) stays put instead of lasting one contact.** You can type your CQ
  message in the Tx6 box on the Operate screen — `CQ DX KR4FQG EM64`, or POTA, NA, TEST, a zone
  number — and it is parsed and sent. What went wrong is that "clear DX call and grid after
  logging" was also wiping that box, so a directed CQ survived exactly one QSO and then went back
  to a bare CQ, with the Classic screen the only way to set it again. That option clears the DX
  call and grid, as its name says, and leaves your CQ message alone. Editing it back to a plain
  CQ is still one change away.

- **DXpedition is no longer counted as something you need.** A DXPED chip sat in the row of need
  icons claiming there was something to gain from a station — including ones you had already
  worked on that band, where it meant nothing at all. It was always a label rather than a reason,
  and it is already shown as an activity marker, so it was being said twice. The Needed board's
  DXped filter is unchanged, as are the POTA and SOTA markers.

- **The "needs QSL" chip now reads LoTW**, which is what actually closes it for awards. A paper
  card closes it too — the tooltip still says so — but eQSL and QRZ never did, and the old
  wording left people wondering whether the eQSL they already had counted.

## [1.8.1] — 2026-08-23

### Fixed

- **Calling a DXpedition no longer gives up after eight tries.** Nexus would stop calling a
  station you had picked yourself once eight overs went unanswered, and then sit silent until you
  clicked it again — which in a pileup is exactly when you least want it to stop. That limit
  exists for a real problem, but a different one: a station that answered you and then went quiet
  mid-contact, which is worth abandoning so your CQ run can move on. That part is unchanged.
  Calling somebody who has not come back is now open-ended, the way WSJT-X does it.

- **A station's callsign is no longer sent back in its shortened form.** When a DX is working
  several callers at once it sends its own call in FT8's abbreviated `<CALL>` form to make room,
  and Nexus copied that form straight into its own replies — so overs went out addressed to
  `<RI1FJL>` rather than `RI1FJL`. It sends the plain call now. Compound calls like `KH8/W1AW`
  still go out abbreviated, because the protocol has no room for them any other way.

- **A DXpedition running Fox mode is understood without turning Hound on.** A Fox packs two
  replies into one transmission, and Nexus could only read that while the DXpedition setting was
  switched on — which also stopped it sending the closing 73 on every ordinary contact. Reading
  the Fox no longer depends on that switch; it applies whenever you are working someone.

- **The Phone waterfall gives the voice more of the panel.** The dial marker sits on the
  suppressed carrier, so on USB your voice always sits to the RIGHT of it — that is correct, but a
  third of the display was being held empty beside the marker to make it read as a line, and that
  empty third made the dial look misplaced. The gap is much smaller now and the signal is wider.

## [1.8.0] — 2026-08-23

### Fixed

- **Linux: the AppImage starts on Wayland desktops again (#138).** It was bundling its own copy
  of `libwayland-client`, which loses to a newer compositor on the host — Nexus opened to a blank
  white window on Fedora 44 and never drew anything. That library now comes from your system,
  where it belongs. Only that one is dropped; its siblings are still bundled, because nothing in
  the report pointed at them and removing them on a guess is how you break somebody else's
  desktop.

- **Nexus no longer gives up on the rigctld you chose because the machine was busy for a moment.**
  Deciding whether a rigctld works meant running it once, and any failure to start it counted
  against the binary — including the failures that say nothing about it at all. A system briefly
  out of process slots, a signal landing mid-call, or the file still being held open by the
  installer that had just written it would all read as "this rigctld is no good", and Nexus would
  quietly substitute its own instead of the one you pointed it at. Those three are now retried;
  a rigctld that genuinely is not there or not runnable still answers straight away.

- **A station that sends you RR73 gets your 73 back.** If DXpedition "Hound" mode had been
  left switched on, every ordinary contact inherited the Fox rule — the QSO ended on the other
  station's RR73 and Nexus sent no parting 73, then switched Enable-Tx off before it could. From
  your side it looked like a normal contact; from theirs you simply vanished, and they went on
  repeating RR73. Hound is a per-DXpedition mode now: it is off again at every launch, you turn
  it on for the DXpedition, and the amber HOUND badge marks the session while it is on. Working
  a real Fox is unchanged.

- **Hold Tx, and the waterfall's RX/TX markers, survive a settings save.** Pressing Hold Tx or
  dragging a marker changed the live setting, but any later save from the Settings window posted
  an older copy back over it and then stored that — so the setting looked as though it had never
  been saved at all. None of the three is editable in Settings; they only travelled in the form,
  so a save could only ever undo them. Restoring a backup still sets all three from the backup.

- **The Needed board respects the New-grid band choice.** With "New grid" set to VHF+, HF grid
  needs were still listed on the board. The band choice reached the roster and the decode rows
  when it was added, but not the board, which had stopped sharing that code path earlier so that
  turning the CW or Phone features off would not hide needs there. Both hold now: the board's own
  mode filters still show everything, and the band choice applies.

- **A contact the run gave up on can still be logged.** If a station answered you, exchanged
  reports and then went quiet — a club station working several people at once does this
  routinely — Nexus stopped calling them after a few overs so your CQ run kept moving. That part
  is right. But it also threw the contact away, so when the station finally came back with RR73
  the Log button said "nothing to log" about a QSO whose reports are in your own ALL.TXT. The
  exchange is kept now, and pressing Log writes it with the contact's own start time. Nothing is
  logged automatically that wasn't before — only you saw them come back, so it stays your call.

- **The Band Activity list marks each period once again, not every decode.** Once about
  three hundred decodes had built up, the dim time-and-band bar that separates one T/R
  period from the next started appearing between every single line. It was comparing each
  decode against one from the far end of the buffer instead of the row above it, so the
  period looked different every time — which is why it began "after a while" and why
  switching the All/CQ filters shuffled it without fixing it.

- **A station answering your CQ makes a sound again.** The alert for somebody calling you
  was being held back for the whole time you were calling CQ — the one stretch where it is
  the only thing you are listening for. It stays quiet once you are into the exchange, which
  is what it was always meant to do.

- **Nexus stops interrogating the system keyring every few seconds.** On Fedora it could
  crash GNOME's keyring service over and over for as long as Nexus was open, once any online
  service had been set up. Nexus was asking the keyring whether each password was still there
  every five seconds; it now asks once and then only when you actually save or clear one.

- **OmniRig gets time to start.** If OmniRig was not already running, Nexus gave Windows a
  second and a half to launch it and gave up — but a cold start takes longer than that, so
  the connection failed for no visible reason and then worked later. Starting another program
  that uses OmniRig first appeared to "fix" it, because that program had done the launching.
  Nexus now waits twenty seconds for a start, while keeping the short timeout for ordinary
  commands so a stuck server still cannot hang the radio.

- **The OmniRig "needs administrator" message points at the fix.** It used to suggest
  starting OmniRig yourself, which can fail for the same reason the launch did. OmniRig does
  not need administrator to reach a radio, so the message now explains how to clear that flag
  — including where Windows hides it when the checkbox already looks clear — and treats
  running everything as administrator as the last resort it is.

- **Nexus starts properly on a system with no regional settings.** On a machine running with
  a plain `C` locale the FT screen came up as an error instead of a cockpit.

- **A busy PC can no longer produce a phantom CAT failure.** A rig reply interrupted by the
  operating system was being read as a dead radio, and on a longer reply it could be cut
  short and accepted as complete.

- **Linux: the AppImage no longer carries its own copy of a system graphics library**, which
  is what left Nexus showing a blank white window on Fedora 44.

### Added

- **Nexus tells you when the radio is armed to transmit at 0% power.** A rig at zero still keys,
  still shows TX, and still looks like a perfectly normal over from where you are sitting — it is
  silent only to the station you are working, so there is nothing to notice. The status lane now
  says NO RF POWER while that is true, and the diagnostic log records it. Nothing is changed on
  your behalf: the power is not raised, not clamped, and no transmission is held back. Worth
  knowing on a Yaesu in particular, which keeps a separate power level for SSB, DATA, CW and AM,
  so a level you set in one mode does not follow the rig into another.

- **The log table shows your Comment, and marks rows carrying a private Note.** Both fields
  could be typed and saved, and neither was ever shown again — the only way to read a note was
  to open a contact you had no way of knowing held one. The Comment now has its own column, and
  a contact with a private Note carries a 📝 you can hover for the full text. (The callsign
  recall card beside the log entry already showed both; that is unchanged.)

- **AM on the Phone screen.** Pick AM beside AUTO/USB/LSB/FM and Nexus commands the rig to AM
  with a 6 kHz filter — an SSB-width filter cuts half of a double-sideband signal away and the
  audio comes out thin. It is offered on the bands AM is actually worked (the windows below
  10 MHz, and 10 m / 6 m and up), not on 20 m where a 6 kHz signal has no room.

  **Your power comes down automatically.** A rig making 100 W PEP on SSB makes about 25 W of
  carrier on AM, so the same drive flat-tops the peaks. AM gets its own ceiling — a quarter by
  default, adjustable — and it is always the LOWER of that and your phone cap, so it can only
  ever reduce power, never raise it.

- **The Phone waterfall has frequencies on it.** It showed you a signal without telling you
  where it was, so clicking to tune was a guess. There is a scale along the bottom now, in MHz
  to the kHz you would dial. It reads absolute frequency rather than an offset from your dial,
  because the question it answers is "where will this click put me". If Nexus does not know your
  dial — no CAT — it shows no numbers rather than invented ones.

- **The Needed list shows the frequency, not just the band.** A rare one on 20 m is a different
  proposition from a rare one on 14.074 — the exact spot frequency is now a column, and you can
  sort by it to see what is worth swinging the dial for. Needs worked out from your log rather
  than spotted have no exact frequency, and say so with a dash instead of an empty cell.

- **Reset all settings to factory defaults.** There was no reset at all: a clean start meant
  finding `settings.json` in a config folder and deleting it — and doing that while Nexus is
  running resets nothing, because the app holds the old configuration in memory and writes it
  straight back on the next save. The new control is in Settings → Radio → Transmit limits &
  sharing, beside Back up. It asks first, and the dialog says what SURVIVES: your contact log
  is untouched, and stored passwords stay in your keychain — clearing those stays a separate,
  deliberate act rather than a surprise buried in a reset.

- **An unanswered CQ run takes a breather instead of holding the frequency.** Nexus now calls
  CQ eight times, waits three minutes, and calls again — both numbers are yours to change in
  Settings ▸ Auto-CQ, and clearing the call count restores the old behaviour of calling until
  you stop it. You are still listening throughout the pause: anyone who calls you is worked
  normally, and answering resets the count, so a run that is getting replies never pauses at
  all. This is a deliberate difference from WSJT-X, which repeats CQ indefinitely.

- **You can record a QSL card that arrived in the post.** The Logbook could already note a
  card you had SENT, but there was no way to say one had come back — even though a paper card
  is one of only two confirmations that count towards DXCC, and the only one no online service
  can tell Nexus about. It is in the QSL menu on each row, and it can be unticked again.

- **"Hide worked" explains itself.** It hides stations you have worked EXCEPT those that still
  fill a need, which is why a B4 chip can survive it — you worked that call on another band,
  and it is still a new slot here. The checkbox now says so.

## [1.7.6] — 2026-08-21

### Fixed

- **The Call Roster shows the full CQ, not just "CQ".** A station calling `CQ DX` looked
  exactly like one calling a general CQ, so you could click to work him and only find out
  from the Band Activity pane that he wants DX and will ignore you. The roster now shows
  `CQ DX`, `CQ POTA`, `CQ NA` and the rest, in a colour that stands out while you are
  scanning, with a note on hover about what it means for answering.

- **A failed OmniRig connection stops telling you to check a serial port.** If Test CAT
  could not read a frequency over OmniRig, the message advised checking the serial port,
  baud rate and CI-V — none of which Nexus uses on an OmniRig connection, as that same
  Settings page says. It now points at what actually matters: which OmniRig slot, and
  whether OmniRig's own window shows that radio online.

- **Special-event callsigns are recognised as CQs again, so double-click works on them.**
  A call like `II7MGBR` or `EN3SUKR` does not fit the standard callsign shape, so FT8 sends
  it in a form that carries no grid — and Nexus was only treating the *compound* kind (the
  ones with a `/`) as real CQs. The rest were read as free text: no CQ chip, and
  double-clicking them did nothing, while the Work button in the Stations list started a QSO
  perfectly well. Both now behave the same way.

- **Your hidden panes and disabled modes survive an upgrade.** If you had turned off modes
  you do not use, or hidden panels with the ⊞ menu, an upgrade could put them all back —
  those two choices were kept in browser storage rather than beside your settings, so
  anything that cleared it took them with it. They now live with your settings, per profile,
  and are included in the backup. A popped-out panel's own layout stays per-window, as it
  should.

- **Switching from CW to the FT screen left the rig on the CW frequency.** FT8 came up on
  wherever CW had been — the mode changed to DIGU correctly, the dial did not. It only
  happened if you passed through Tempo on the way, which is why it looked intermittent:
  Tempo is a digital mode and asserts the rig mode, but it keeps its own band picker's
  frequency, and it was being counted as though it had already moved the dial for you. The
  FT screen then thought there was nothing to do.

- **A hardware CW keyer learns your speed before you send anything.** With a WinKeyer, the
  speed slider only reached the keyer once you had sent something — so after launch the
  paddle ran at whatever speed the keyer itself was set to, and moving the slider did
  nothing until you typed a character. The keyer is now opened and told your speed as soon
  as it can key.

- **The filter width the radio actually took is now checked, not assumed.** Some rigs accept
  a mode change with a filter width, answer "done", and quietly keep their own filter — so
  FT8 ended up on a 6 kHz SSB filter with nothing saying so. Nexus now reads the width back
  after a mode change and re-asserts it once if the radio kept its own, and if the second
  attempt is ignored too it tells you the actual width instead of implying success. Rigs
  that round to the nearest filter they own are left alone: asking for 3 kHz and getting
  2.7 is the radio doing its job, not a fault.

- **A manual notch you can actually place, and a depth for the speech processor.** The Notch
  button was driving the radio's *automatic* notch — the one that hunts a carrier down by
  itself — which is not what most operators mean by the word. The manual notch, the one you
  park on a whistle by ear, is now there too, with a frequency slider to put it where the
  whistle is. COMP gained the control it was missing: how hard the compressor works. Each
  appears only if your radio reports it, so nothing grows a slider with nothing behind it.

### Added

- **The dial is marked on a native RF panadapter.** If your radio streams its own spectrum
  (Icom CI-V, FlexRadio), the tuned frequency now has a line and a DIAL label on it. It is
  drawn only where the dial genuinely is: on a rig in FIXED scope mode, where the span is a
  band segment and the VFO sits wherever you tuned it, the line lands off-centre — and if
  the dial is outside the displayed window it is not drawn at all rather than pinned to the
  edge, which would say something untrue.

- **PSK and Tempo chapters in the manual.** PSK31/QPSK31 shipped four releases ago with no
  chapter and Tempo never had one. Both now cover the tour, the workflows and the honest
  limits — and a test now fails the build if a shipped section has no chapter, so the next
  mode cannot reach a release undocumented.

- **AGC now offers AUTO and OFF, and stops reporting them as Mid.** The Phone and CW cockpits gave
  three AGC settings — Fast, Mid, Slow — while the radios have five: an FT-710 offers AUTO and OFF
  on its own front panel. Worse than missing, they were *misreported*: a rig sitting on AUTO, which
  is where many operators leave it, displayed as **Mid**, and so did AGC switched off, because
  anything Nexus did not recognise folded to "mid". So the cockpit could state a setting the radio
  was not on. AUTO now appears to the left of Fast and OFF to the right of Slow, and a read-back of
  either shows what the rig is actually doing. Offered for every rig rather than guessed at:
  Hamlib does not report which AGC constants a backend accepts, so a rig that refuses one says so
  and the read-back shows what it really did.

## [1.7.5] — 2026-08-20

### Added

- **Deutsch.** Nexus can now run in German — the first language it has ever offered. Pick it in
  Settings ▸ Appearance ▸ Language, or just run it on a German Windows and it starts in German
  by itself. Anything not yet translated appears in English rather than blank, so nothing can
  break by being missing.

  **Nothing technical is translated, and that is deliberate:** frequencies, signal reports,
  callsigns, grid squares, band and mode names, Q-codes, RST, POTA/SOTA references and ADIF
  field names read the same in every language. In particular a frequency never picks up a German
  decimal comma — 14.074 is 14.074, and a test fails the build if a comma ever appears in a
  translated number. Every screen is covered — about 4,600 phrases — except the transmit
  controls themselves (Stop TX, Tune, the TX arm switch, ATU and the TX/RX indicator), which
  stay in English until they can be changed as their own reviewed step. Those are the controls
  that stop a transmission, and they are not something to get wrong in a language nobody here
  reads.

- **Find a station in a crowded list.** The Stations panel has a search box beside its
  filter chips, and it takes wildcards: type `PA*` for every PA prefix, `ON4*` for every
  ON4, or both at once — several terms mean "any of these". `?` fills in exactly one
  character, and a plain word like `4FD` still matches anywhere in a call. It narrows
  whatever the chips are already showing rather than replacing them, the count beside the
  title tells you how many of the total you are looking at, and **Esc** clears it. The same
  wildcards now work in the Spots panel's search.

- **Tune, ATU and RF power in the PSK and RTTY headers.** Both cockpits were missing the
  controls every other one puts in the same place, so an operator who tuned up in Phone and
  switched to PSK found the button gone. In PSK this is the mode's one real hazard: set the
  drive against a Tune carrier, below where ALC starts to move, and your signal stays clean —
  overdriven PSK31 splatters, and it looks fine on your own waterfall while it does it. RTTY
  keys the carrier for the whole over, so it wants running well under the rig's SSB rating.
  The ATU button appears only if your rig actually reports a tuner.

- **Periodic logbook backups, in a `backups/` folder beside your log.** Nexus now keeps dated
  snapshots of `log.adi`: at most one a day, only when the log has actually changed, **plus**
  an immediate copy any time a save is about to make the log *smaller* — the one shape of
  failure that has ever cost QSOs here. The folder is bounded three ways so it cannot creep:
  the ten most recent snapshots, a 64 MB ceiling over the whole folder, and the one-a-day
  rule. Oldest go first. The original `log.adi.bak` anchor is separate and is never rotated or
  deleted. Snapshots are taken when the log is **saved**, never when it is opened, so however
  large your log gets, launching Nexus does no extra disk work for this.

- **A diagnostic log you can send us.** Nexus now keeps a plain-text record of what it did:
  `nexus-diag.log`, in the same folder as `ALL.TXT` and the crash report
  (`%LOCALAPPDATA%\Nexus` on Windows) — the Reveal button beside the ALL.TXT path in Settings
  opens it. Timestamped, human-readable lines covering the startup steps, the CAT and audio
  device open and any failure, updater checks, and crashes. Until now Nexus wrote nothing at
  all about its own health, so "it won't start" reports arrived with nothing to look at. It
  bounds itself: two files, about 8 MB in total worst case, and the older one is simply
  renamed aside rather than rewritten, so a big log never slows a launch down. Passwords, API
  keys and tokens are masked before anything is written — the file is meant to be attachable
  to a public bug report. Settings ▸ Logging & Connectors ▸ Integrations & Feeds names it and
  has its own Reveal button, and the settings search finds it under "diagnostic log", "log
  file" or "troubleshooting".

### Fixed

- **OmniRig failing with "requires elevation" now says what to do about it.** If Windows
  refuses to start OmniRig for Nexus (`0x800702E4`), it is because OmniRig is set to run as
  administrator and Nexus is not — nothing in Nexus is missing or misconfigured. The message
  now says that, and tells you the fix: start OmniRig yourself and leave it running (Nexus
  attaches to the copy already up), or clear "Run this program as an administrator" on
  OmniRig.exe, or run both as administrator.

- **The Connection help now says how a LAN-connected Icom gets in.** Icom's network protocol
  is its own, so the route is wfview (or RS-BA1) against the radio and Nexus pointed at
  wfview's rigctld server with Rig Model **NET rigctl** — which has always worked, and was
  written down nowhere.

- **Virtual COM ports show up in the rig picker on Windows.** If you run SmartSDR CAT, com0com,
  or any virtual serial pair, its ports were missing from the port list while your real USB
  adapters showed up fine — so the only way to use one was to type the name in by hand. The
  Windows port scan asks for two device categories that virtual pairs do not belong to, and its
  backup lookup was discarded whenever you had more real adapters than virtual ports. Both lists
  are now merged, so every port either one knows about is offered, once. (#117)

- **Saving in Settings no longer ends the contact you are working.** Pressing **Save** while a
  QSO was in flight threw the contact away — the sequencer that owns it was reset back to Chat,
  so the QSO never completed, nothing auto-logged, and the **Log** button then did nothing at
  all: no logbook entry, no QRZ upload. A save now leaves a contact in progress exactly where
  it was, and it still logs. The mode is only reset where that was ever the point — turning the
  Field Day master switch on or off, which still enters and leaves Field Day as before. A save
  is still a heavyweight act in every other respect: queued transmissions are dropped and the
  transmit cycle is re-derived, so it is still not the way to change one setting mid-QSO.
  Reported by kr4fqg ([#100](https://github.com/kd9taw/Nexus/issues/100)).

- **CW speed slider now reaches a WinKeyer.** Moving the speed slider between overs did
  nothing on the WinKeyer back-end: the speed was only ever sent to the keyer at the
  moment a word left the queue, so a hardware keyer sat on its own front-panel pot or
  power-on speed and the number in the cockpit was a number Nexus alone believed. The
  speed now goes to the keyer as soon as you move the slider — idle, or part-way through
  a message — and is re-sent whenever the keyer's port opens, so one plugged in late or
  power-cycled comes back at your speed. Switching from the CAT keyer to a WinKeyer at
  the same speed pushes it too; the two back-ends no longer share one "already told it"
  record. The CAT keyer is unchanged and still sends `KEYSPD` at the start of a send.
  Thanks to **swinn** for the report (#135).

- **The waterfall's frequency scale follows the RX marker when you are zoomed in.** On any
  zoom level other than Std or Full, the numbers along the bottom of the waterfall were set
  once, when the screen opened, and never moved again — so a display that came up before the
  radio said where you were listening kept labelling a 600 Hz zoom 200–800 Hz however far you
  tuned, and the picture no longer agreed with the scale under it. A zoom is meant to be that
  many Hz wide, centred on the green RX marker: it now re-centres whenever you move the
  marker — a waterfall click, a double-clicked decode, netting RTTY or PSK — and the labels
  travel with it. Std and Full are fixed views of the whole passband and are unchanged.
  (#115, reported by akhepcat)

- **The alert band scope now governs the need ICONS, not just the sound.** Set
  Settings ▸ Spots & Alerts ▸ **New grid** to *VHF+ (6 m and up)* and 20 m went quiet — but
  every FT8 row on the Call Roster and in Band Activity still wore a GRID chip, in both
  Roster and Classic layouts. The scope only ever reached the beep and the toast; the icons
  were painted from a different path that never saw the setting. It does now, and the same
  goes for **New DXCC** and **Rare grid 💎** — one choice per need type, covering both what
  you hear and what you see, on the docked window and on a torn-off Operate panel alike.
  Nothing else about the row changes: a station that is a new band-slot *and* a new grid
  keeps its BAND chip on HF and loses only GRID, a call on your watch list still flags
  everywhere you hear it, and a need is judged by the band it was heard on — so a 6 m grid
  stays marked on a roster you are reading while parked on 20 m. Where the band cannot be
  worked out at all, the icon is shown: a missing chip is worse than an extra one.

- **A logbook with accented or non-English text in it could load as EMPTY — and the next save
  wrote that empty log to disk.** If your `log.adi` held a single byte that wasn't plain
  English — a Greek name, a German umlaut, a French accent in NAME, QTH or COMMENT, which is
  exactly what a Greek, German or French Windows writes — Nexus failed to read the file,
  treated it as an empty logbook, skipped its own safety copy *because* it looked empty, and
  the next save rewrote `log.adi` from zero records. Every QSO, gone, silently, with no
  backup. Nexus now reads the log as raw bytes and never fails a load over an encoding: the
  contacts all load, and the one-time safety copy is taken from the original bytes. Found
  while investigating a Greek-Windows launch report. If this bit you, `log.adi.bak` beside
  your log holds the original.

- **The decode panes remember ten times as much, so you can actually scroll back.** Both panes
  kept 300 rows — but they keep the WHOLE band and filter at display time, so on a busy evening
  the Rx Frequency pane spent all 300 on signals it never shows and could only look back about
  four minutes. The two or three rows on your own frequency, which is the entire point of that
  pane, were thrown away with the rest. The store is now 3,000 rows: roughly ten minutes of a
  packed band in Band Activity, and hours of your own frequency in the Rx pane. What gets drawn
  at once is unchanged, so nothing renders slower — measured at 0.12 ms per redraw with the
  store completely full.

- **The Hold Tx button's tooltip described the wrong thing.** It said Hold keeps your TX offset
  fixed "when you click the waterfall to set RX" — but a plain waterfall click has not moved TX
  since 1.0, Hold or no Hold. What Hold actually governs is the double-click: work a station with
  Hold off and your TX moves onto their frequency, which is WSJT-X's behaviour and is what you
  want most of the time; turn Hold on and your TX stays exactly where you set it with a
  right-click. The tooltip now says that, in the Operate strip and in the top bar. No behaviour
  changed.

- **The QSL chip now says which confirmations count.** The chip on a worked-but-unconfirmed
  station read "a QSL from this station would close it", which left you to guess whether eQSL or
  QRZ would clear it. They will not: Nexus counts a LoTW match or a paper card — the same grade
  the awards screens count, so the chip and your award progress can never disagree — and the
  tooltip now says exactly that. (Asked on air: the chip is easy to read as QSO at pill size,
  and relabelling it LoTW would have been wrong for the card half.)

- **Nexus could arrive in FT8 with the transmitter already armed.** Entering PSK31, CW, Phone or
  RTTY arms transmit — correct for those modes, which are a live key or mic, and arming by
  itself sends nothing. But nothing disarmed on the way back, so returning to FT8 left the
  transmitter armed from the previous mode, and in FT8 an armed transmitter is what turns
  selecting a station into an immediate call. Reported on air: PSK31 → FT8 → pick 20 m → "it
  started transmitting on its own." Leaving a manual mode now lowers the arm switch, so FT8 is
  armed only by Monitor, a double-click, or Call CQ — the three gates it always claimed to have.
  A queued CW or RTTY over waiting for you to come back is still held, exactly as before.

- **A Windows update no longer throws away what Nexus was holding in memory.** Installing an
  update on Windows hands off to the installer and ends the app then and there — so the
  conversation history, the Field Day log, a propagation opening still in progress and your
  window positions were never written, and the diagnostic log lost the lines covering the
  update itself. All of it is now flushed to disk immediately before the handoff. The
  transmitter is untouched by this: installing is already refused while you are transmitting,
  tuning, in a QSO, running or merely TX-armed. macOS and Linux were never affected — there
  the app restarts through its normal shutdown.

- **Nexus could fail to start with no window, no error and nothing to send.** If the Microsoft
  Edge WebView2 runtime — the component Nexus uses to draw its window — was missing, damaged or
  had a corrupt cache, the app exited without a trace: no window, no message, no crash file.
  That is the Greek-Windows report, and it is why reinstalling did not help. Nexus now says so
  in a dialog that names WebView2 and gives the repair steps, writes the reason to the new
  diagnostic log, and repairs the commonest case itself: the WebView2 cache folder is set aside
  and startup is retried once before giving up.

- **Frequencies and coordinates typed with a comma decimal separator were read wrong.** On a
  Greek, German, French or any other comma-decimal Windows, typing `14,074` into the cluster
  spot box, a memory channel or the APRS beacon latitude produced a wrong number — the APRS
  case put bad position data on the air. Every place Nexus takes a typed number now accepts
  both `14,074` and `14.074`, and rejects what is not a number instead of storing it.

## [1.7.0] — 2026-08-18

### Added

- **OmniRig rig control (Windows).** The **Connection** dropdown in Settings ▸ Radio ▸ Rig
  & CAT gains **OmniRig** — VE3NEA's rig-control server, the one much of the Windows
  logging and contest world already runs. Pick it and your radio is configured once, in
  OmniRig, and every program shares it instead of fighting over the COM port. Because
  OmniRig owns the rig type, the port and the baud, Nexus stops asking for them: the only
  thing it needs is **which** radio — `RIG 1` or `RIG 2`, matching OmniRig's own two tabs,
  defaulting to RIG 1, and per radio, so a two-radio station can put one on each. Dial and
  mode follow both ways, exactly as on a serial rig. **PTT:** leaving PTT Method on `CAT`
  keys through OmniRig with nothing else to set up, and the other choices are unchanged
  and deliberately independent of it — pick `RTS`/`DTR` with a PTT Serial Port and Nexus
  asserts that hardware line itself, which is how many operators run. It is Windows only
  (OmniRig is a Windows program), so on macOS and Linux the choice is shown greyed out
  with that reason rather than hidden. If OmniRig isn't installed Nexus says so plainly,
  and if it is running but the rig is off, on a busy port, or not answering, the CAT
  status shows OmniRig's own words for it instead of a bare "no reply". See the
  [rig setup guide](docs/rigs/index.md).

- **QPSK31.** The PSK screen's mode picker now offers QPSK31 beside PSK31 — same speed,
  same feel, but with error correction: the four-phase signal carries a convolutional
  code, so the flutter or static crash that prints a wrong character in PSK31 is simply
  corrected. Receive and transmit both work — pick QPSK31, click a trace, and the same
  macros, continuous TX and stop controls apply. One thing to know on the air: QPSK31
  cares which sideband the other station transmits on. Nexus runs the standard (USB)
  polarity; if a station warbles but prints garbage, click the Rev toggle beside the mode
  picker. Mode and polarity switch only between overs — never mid-transmission.

- **PSK31 transmit.** The PSK screen now works both ways: type a line and press Enter for
  a one-shot over, fire the F1–F4 macros (CQ / Answer / Exchange / 73), or click TX for
  continuous transmit — stay keyed, idle on the classic PSK31 reversals, and what you type
  goes out as you type it, mixed case and all. TX keys at the same spot you netted the
  decoder (click the trace, answer on frequency), the rig is put in its data mode
  automatically (USB-side on every band, the PSK31 convention), and a band picker offers
  the standard watering holes — 14.070, 3.580, 7.070 and friends — showing only the bands
  your license can key. Stop TX, the Esc/Stop button and Esc all cut a transmission
  instantly, leaving the section or QSYing out of your privileges unkeys within a tick,
  and a hard 10-minute ceiling bounds a continuous over no matter what. One deliberate
  courtesy: Nexus transmits at a modest drive and the dock reminds you to keep the rig's
  ALC near zero — an overdriven PSK31 signal splatters into the neighbors (IMD).

- **Export a date range from the logbook.** Compact from/to date pickers now sit beside
  Export ADIF — set either or both and the ADIF/CSV export carries only the QSOs in that
  UTC date range, which is what a POTA activation or one weekend's contest needs. Leave
  them empty and the export is the whole log, exactly as before. The per-operator export
  is deliberately untouched: it is the submission path, and a leftover date silently
  truncating an uploaded log would be the worse surprise. (#98)

- **PSK31 receive.** A new PSK screen on the Digital rail decodes the classic narrow-band
  keyboard mode — open it, tune a watering hole (14.070 is the busy one), click a warble
  trace on the waterfall and the text prints, with faint characters marking doubtful copy.
  The receiver starts by itself when the screen opens (turn that off in Settings ▸ Digital
  ▸ PSK; stopping it by hand is remembered for the session), the click nets the decoder
  rather than the rig, and a gentle AFC (never more than ±25 Hz) rides small drift.
  Transmit ships in this release too (see the PSK31 transmit entry above), and so does
  QPSK31 (its own entry above).

- **Every Mac release is now checked harder before it ships.** The release pipeline
  proves the signed app still carries the microphone entitlement and usage prompt (the
  exact defect that silently shipped a dead mic in 1.5.0–1.6.1), and it unpacks the
  self-update bundle, matches its version against the DMG and verifies its signature
  with the same key every installed copy uses — a broken or missing update bundle now
  fails the build instead of quietly publishing a release Macs could never update to.

### Fixed

- **FLEX-8400 and FLEX-8600 owners can find their radio in the list.** The rig picker, the
  detected-radio row and the Flex page all said "FLEX-6xxx", so the current flagship line
  appeared to be unsupported. It always worked — SmartSDR CAT presents the same commands for a
  6000 and an 8000, and Nexus drives both through one profile — but nothing said so. The entry
  is now "FlexRadio FLEX-6xxx / 8xxx (SmartSDR CAT)".

- **Nexus now notices when the radio is keyed and no RF is coming out.** Every check Nexus had
  was on the *unkey* side: it made sure the transmitter came down. Nothing ever asked whether
  anything went out in the first place. A transmit inhibit, an antenna port that only receives,
  an amplifier interlock, a transmit profile pointed at an audio source that is not running —
  each of them looks like a completely normal over: PTT is accepted, the meters move or don't,
  the QSO is logged, PSK Reporter is told, and the first evidence is nobody coming back to you.
  If your radio reports forward power over CAT, Nexus now watches that reading during an FT8,
  RTTY, PSK or SSTV over, and after a couple of seconds of zero watts it says so in the radio
  status line. It is a **message and nothing else** — it never stops or blocks a transmission.
  It stays quiet on radios that do not report forward power (most do not), during a tune-up,
  through the first moments of key-down, on phone and CW overs (silence between words and
  between elements is normal), and it never repeats inside one over or fires once the radio has
  been seen making power.

- **Phone tells a FlexRadio operator when native DAX has taken their microphone.** Switching
  on **Flex native DAX audio** — the early-access toggle you would turn on for FT8 — tells the
  radio to take transmit audio from DAX. That is a *radio-wide* setting, not a Nexus one: while
  it is on, your Flex's microphone is disconnected on every slice, in every program, SmartSDR's
  own MOX included. Turn it on for digital, pick up the mic later for an SSB contact, and you
  transmit silence with nothing anywhere saying why. The Phone screen now shows a **mic off
  (DAX)** marker beside the frequency whenever native DAX audio is actually running, and says
  what to do about it: turn Flex native DAX audio off in **Settings ▸ Radio ▸ Rig & CAT** and
  the mic comes straight back. It appears only while the native audio really is live — not
  merely because the toggle is on with no radio address, or after the audio path has already
  fallen back to your sound card. The Flex setup guide gained a matching
  [Phone (SSB) section](docs/rigs/flexradio.md).

- **CW on 160, 80 and 40 m now actually puts an SDR radio into CW.** Below 10 MHz Nexus asks
  the radio for **CW-L**, which is the right thing to ask for and what most radios call it.
  Four rig profiles do not offer it under that name — **FlexRadio SmartSDR CAT**, **SmartSDR
  native**, **PowerSDR** and **Thetis** — and they refused the request outright. Nexus then
  gave up and left the radio in whatever mode it happened to be in *while the keyer went right
  on sending*, so a CW macro could go out of a radio still sitting in USB or a data mode, on
  the three lowest CW bands, with nothing on screen naming the cause. Nexus now notices the
  refusal and sets plain **CW** instead. It is the same signal on the same frequency —
  CW-reverse only changes which side of the carrier you listen on — so nothing about your
  transmission changes. The Flex setup guide gained a [CW section](docs/rigs/flexradio.md)
  covering the keyer choices on that radio.
- **Dragging text on an SSTV picture makes it stay where you put it.** Text you dropped snapped
  straight back to the middle. The drag itself was fine — the picture is redrawn from scratch
  many times a second, and each redraw quietly restored the text to where it had last been
  saved, which for a fresh caption is the centre. Letting go then saved that restored position,
  so the snap-back stuck. Nudging with the arrow keys was never affected and still isn't.

- **Rotators: the baud now comes from the model, so five of them work for the first time.**
  If you own a **SPID Rot2Prog or Rot1Prog**, an **Idiom Press Rotor-EZ**, a **Hy-Gain
  DCU-1/DCU-1X** or a **Green Heron RT-21**, your rotator has never answered Nexus — and
  this is why. Nexus gave *every* rotator model the same 9,600 baud and told you in the
  tooltip that was right, then forced it on the control daemon over the top of the rate
  Hamlib knows your controller actually uses: 600 for the Rot2Prog, 1,200 for the
  Rot1Prog, 4,800 for the Rotor-EZ, the DCU-1 and the RT-21. At the wrong line rate a
  controller simply ignores you, which looks exactly like a dead cable or broken hardware.
  Picking your model now fills in the rate its own backend declares, read out of the
  Hamlib that ships in the installer rather than typed from a manual; where a model
  genuinely accepts a range (the GS-232 family, EasyComm, SPID MD-01/02) your own setting
  is left alone, as it should be. **If your rotator has never worked, re-pick your model
  in Settings ▸ Radio ▸ Rotator** — and if the saved number cannot work, the hint under
  the box now says so in words, with the number to use.

- **A rotator command that got no answer was reported as success.** Nexus treated silence
  from the rotator daemon as "done", so the compass slew, the ↗ point-at-call and a whole
  satellite pass could report that the antenna had moved while it sat still — and the
  "the rotator stopped answering, point it yourself" warning could never fire, because
  every unanswered command was counted as a good one. Silence, a timeout and a hang-up are
  all failures now, and the daemon's own refusal reaches you with its reason. The wait
  before giving up is matched to what Hamlib itself allows your model, instead of being
  shorter than it for almost every rotator in the list.

- **A rotator daemon that died at launch was logged as "launched".** The commonest rotator
  failure — a port that is not there, a port another program already has, a model number
  the bundled Hamlib does not carry — kills the daemon within milliseconds, and Nexus
  reported success and then went quiet forever; re-saving Settings did not bring it back.
  It is noticed now, at launch and afterwards, restarted, and reported with **Hamlib's own
  words for what went wrong** in the Connections log, where before there was nothing to
  read at all.

- **A Green Heron RT-21 refused the last tenth of a degree of the compass.** Bearings from
  359.95° up were sent as `360.0`, which that controller's Hamlib backend rejects outright
  (it declares a 359.9° ceiling). They are sent as 0.0° now — the same bearing, and one
  every rotator accepts.

- **"Allow flip" did nothing.** The setting promises to take a high satellite pass by
  running elevation past 90° instead of swinging the mast 180° at the top of the pass, and
  the condition it was gated on could never be true — so the mast raced round on every
  high pass with the box ticked. It works now: above 85° peak elevation, a mount whose
  owner has said it can go over the top keeps the antenna on the bearing the bird rose on
  and runs elevation up through 90° and out the far side.

- **A failed park at the end of a satellite pass drove the antenna FLAT.** If the park
  command was refused, the fallback commanded elevation zero — laying a dish or a long
  boom into the wind, which is the thing a park position exists to avoid — and said
  nothing about it. The fallback is now azimuth-only for rotators that genuinely have no
  elevation axis, and for everything else a park that did not complete is reported and the
  antenna is left where the pass ended.

- **A stuck or jammed mast was invisible during a pass.** Nexus never asked the rotator
  where it actually was, so a controller that accepted every command and then stopped
  moving — a jam, a hit stop, a slipped belt, a box left in local — tracked "perfectly" for
  the whole pass. The position is read back now, and a mast that is neither on target nor
  moving is called out in the Connections log. A rotator that stopped answering mid-pass
  is logged there too, instead of only appearing on screen for whoever was watching.

- **A rotator that cannot report its position no longer loses its controls.** Some models
  genuinely have no read-back — the Hy-Gain DCU-1 is one — and the Connect rotor pane used
  to delete itself entirely when the readout failed, taking the compass rose, the typed
  bearing and the **STOP** button with it. It stays, with "—" where the needle would be.

- **Rotator settings live in the Rotator section.** The model, port and baud were filed
  under Rig & CAT, so the "Rotator not answering" chip — the one affordance the app has for
  this failure — opened a section containing neither, and searching Settings for "rotator"
  found only the pointing manners. Clicking the chip now lands on the model and the port.

- **The rotator list: a wrong entry out, six models in.** "EA4TX ARS (az)" was Hamlib's
  **parallel-port** backend offered with a serial port and a baud box — it could not work
  as presented, and the brand name steered ARS-USB owners away from the GS-232 generic
  entry that does (which now says so). Added, all of them models operators actually own:
  SPID MD-01/02, Prosistel Combi-Track az+el, Hy-Gain DCU2/DCU3/YRC-1, DF9GR ERC, AMSAT
  LVB Tracker, and the Kenpro GS-23/GS-232. Entries now say **(az)** or **(az/el)** where
  the backend declares it, so an az/el owner is not steered onto the azimuth-only variant
  of their own manufacturer. And a saved model that is not in the list shows its number
  instead of an empty box.

- **FlexRadio, the ORDINARY setup: your Flex settings stay put, the radio's address is
  kept, and a switched-on feature says what it needs.** Unlike the two entries below —
  which are about the opt-in "early access" toggles nobody is required to turn on — this
  one is the everyday, field-verified FlexRadio configuration: SmartSDR CAT, DAX audio,
  the way the guide sets you up. The worst of it was silent: the Flex radio address and
  the two native toggles were stored once for the whole station instead of per radio, so
  two Flexes could never both be configured, and — the expensive part — going into
  Settings to configure a *different* radio and pressing Save wiped the Flex settings of
  the radio you were not even looking at, while Save reported success. They now travel
  with the radio they belong to, and an existing address is carried over on first launch.
  Alongside that: the setup wizard threw away the Flex address it had just discovered, so
  a Flex set up through the wizard arrived in Settings with both native features offered
  and no address for them to use; switching a native feature on *before* filling the
  address in left it permanently dead — typing the address afterwards did nothing at all,
  and neither did re-picking the radio, so only restarting Nexus (or toggling the feature
  off, saving, on, saving) brought it back; and turning one on with no address set did
  nothing and said nothing, which now reads as a plain message telling you which field to
  fill in. Two more: the one-click "found a Flex" button applied a Windows-only SmartSDR
  CAT address on macOS and Linux, where that program cannot be installed — it now applies
  only what is true on your platform and tells you what to enter; and two radios pointed
  at one network CAT address now warn, exactly as two radios on one COM port already did.
  Settings' radio list also labels its addresses now (CAT, Flex radio, CAT helper port) —
  a network Flex has three, and they were bare numbers.

- **A radio moved out from under you is finally visible in the FT8 screen.** Change the
  mode at the radio — in SmartSDR, from a Maestro, or with the front-panel knob — and
  Nexus keeps its own idea of the mode, by design: the read-back is a display value and
  never overrides what you commanded. Every screen that could show the disagreement did,
  except the one that transmits unattended for hours. The Operate strip now shows a "rig:
  …" chip when the radio disagrees, and flags a receive filter far too narrow for an FT8
  window — a CW filter left in, or a slice narrowed at the radio, throws away most of the
  band with nothing on screen to explain the quiet.

- **A CAT level the radio refuses is no longer re-sent fifty times a second.** RF power,
  mic gain and noise reduction each kept re-issuing a setting the radio had rejected, on
  every 20-millisecond tick, for the rest of the session — a continuous stream of CAT
  traffic on the one thread that also draws the waterfall, times the FT8 slot and holds
  PTT. That is the "waterfall hangs for a moment, then it's fine, then it lags again"
  report, at many times the rate of the case already fixed for the DSP controls. Nexus now
  tries once, gives up, and says the radio would not take it; moving the slider tries
  again, as does any CAT reconnection. The same discipline reached two more places: the
  750 ms rig read-back now has a time budget, so a radio at the end of a slow or remote
  link degrades how often it is read instead of stalling the loop that has to stay
  responsive to Stop TX, and the S-meter re-check backs off on a radio that has no CAT
  S-meter at all instead of costing three blocked reads every 30 seconds, forever. Every
  CAT radio benefits, not only a Flex.

- **The CAT helper is watched now, and split is put back the way it was found.** If
  rigctld — the helper process Nexus talks to your radio through — died, nothing noticed:
  CAT stayed dead until you re-saved Settings, and because the unkey command travels the
  same path, a crash mid-transmission took away the ability to stop transmitting. Nexus
  now spots it, restarts it, unkeys through the fresh connection and tells you it
  happened. Separately, ending an over with Split Operation set to "Rig" wrote split OFF
  and left VFO B where Nexus had put it — cancelling a split you had set yourself at the
  radio (or that another program sharing the radio had set) and clobbering its transmit
  frequency. Both are restored to what they were. Also: when the radio reports it is
  already transmitting under another program's control while Nexus is armed, that is now
  said out loud instead of passing unmentioned.

- **FlexRadio native audio and panadapter (early access): Nexus now shares the radio
  properly, listens to the right slice, and comes back after a network blip.** These are
  the opt-in "Flex native DAX audio" and "Flex native panadapter" toggles in Settings —
  both off by default and still unverified on real hardware, so nothing here changes
  anything for an operator who has not turned them on. The theme is that the code assumed
  it was the only client on the radio, and a Flex almost never is: SmartSDR's own window is
  usually running, and a Maestro, N1MM or a second Nexus window can be too. What that cost:
  the native panadapter would latch onto *any* panadapter on the radio — including
  SmartSDR's own — then retune it, rescale it and delete it on the way out; native audio
  would adopt whatever DAX stream held channel 1, even another program's, and remove it on
  teardown. Nexus now creates and steers only its own, and removes nothing it did not
  create. Which slice you hear was the other half: audio followed whichever slice had focus
  while the dial, the waterfall and the log all came from the slice your CAT connection
  drives, so on a two-slice radio you could be decoding one slice and logging another with
  nothing saying so. Audio now follows the slice your CAT port names, re-binds when you
  switch at the front panel (it never could before), and the S-meter shows your slice's
  signal instead of whichever slice reported last. Also fixed: a dropped audio packet used
  to be spliced over silently, shifting the timing of every decode after it — the gap is
  now filled with its own length of silence and named in the log; ordinary network
  reordering could leave the native waterfall completely blank; a lost connection or a
  radio reboot ended native audio and the native panadapter for the rest of the session —
  both now re-dial with a backoff; native audio that died mid-session left you deaf with no
  warning, and now falls back to the sound card and says so, as it already did for audio
  that never started; and the RX Gain slider, which did nothing at all on native audio, now
  boosts a quiet slice exactly as it does on the sound card.

- **FlexRadio native audio (early access): the transmit half is fixed, and it no longer
  freezes the app when the radio is unreachable.** This is the opt-in "Flex native DAX
  audio" toggle in Settings — off by default, and still unverified on real hardware, so
  nothing here changes anything for an operator who has not turned it on. With it on, both
  directions ride the network, transmit included (the toggle's old "RX-only" wording was
  simply wrong and has been rewritten). What was wrong with that transmit path: the Pwr
  slider did nothing — the radio got full-scale audio wherever Pwr sat, Pwr at 0 included,
  which is the drive-discipline problem the control exists to prevent; the same over was
  sent twice, over DAX *and* out the configured sound device, so operators using the
  one-click "Pair DAX audio" setup fed the radio two copies and anyone with speakers
  selected heard every over in the room; a 12-second over was flung at the radio in a few
  milliseconds instead of streamed in real time; and if the radio refused or was slow to
  answer the request that sets the transmit stream up, Nexus switched the rig's audio to
  DAX anyway and never switched it back — leaving the microphone dead even after Nexus
  had quit. All four are fixed: one route carries an over, at the drive you set, paced in
  real time, and the mic is always put back. Nexus also puts your slice's own DAX channel
  back the way it found it now, instead of leaving it moved to channel 1 for good. Stop TX
  cuts a native over the same way it cuts any other. Finally, a Flex address that does not
  answer — a stale home LAN IP used from away, a VPN down — no longer freezes the app for
  up to two minutes when the feature is switched off, and a Flex that reboots or drops the
  connection no longer pins a CPU core.
- **Test CAT stopped speaking Icom to Xiegu owners.** When Test CAT walks the CI-V rates
  to find the one your radio answers on, its verdict used to quote an Icom rig menu
  (MENU ▸ SET ▸ Connectors ▸ CI-V), explain that your USB port was following a [REMOTE]
  jack, and finish by telling you to install Icom's USB driver. None of that exists on a
  G90, X6100, X6200, X5105 or X108G — those radios reached this diagnostic when it was
  broadened to every CI-V rig, and the advice was not broadened with it. A Xiegu now gets
  the one cure that is certain (set Baud here in Settings, and the found rate is named),
  the right driver pointer, and no invented menu path. Icom verdicts are unchanged.

- **Detect no longer badges both of an X6100/X6200's serial ports "use this one".** Those
  radios present two USB serial ports and CAT answers on only one of them, but the
  disambiguation was written for Icom's dual-port cable and matched both — so Detect
  showed two identical-looking rows for one radio and half of them sent you to the port
  that opens cleanly and then returns nothing forever. Exactly one row is now badged, and
  Auto-test sweeps that port first instead of burning a full baud sweep on the dead twin.

- **An FT8/FT4 decode that overran the 15-second period could replay a transmission into
  the wrong slot.** Field report, visually confirmed at the rig: with Tx 1st/even set, the
  station's own CQ keyed real RF a few seconds into an odd slot. On a machine that stalls
  long enough for a period's decode to outlive the period itself, the late result re-ran
  the transmit decision with its original slot number — which still passed the even/odd
  check, because that check trusts the slot it is handed. A decode result whose slot is no
  longer the one on the clock now folds its decodes and reports its period but can never
  key. Latent since 0.13 on every platform; it takes a badly stalled machine (a long AV
  scan, heavy swapping) to trigger.

- **The Setup-health RX-audio light can finally say "No RX audio".** Its threshold was
  written for the wrong dB scale, so with a radio configured the light was green even on a
  stone-dead input — the one state it existed to catch. A silent capture now shows
  "No RX audio" and points at the audio device settings.

- **A capture that delivers only pure silence now says so — with the Mac's likely cause
  named.** On macOS, denying the microphone permission doesn't error and doesn't stop the
  audio stream; it just delivers exact digital silence forever, which every health check
  read as "capture alive". After 15 seconds of that, a banner now explains it — and on a
  Mac adds: if the RX meter never moves, check System Settings ▸ Privacy & Security ▸
  Microphone. It clears itself the moment real audio arrives.

- **"System default" audio input now notices its device vanishing on macOS.** With the
  out-of-box device selection, unplugging the rig's USB audio (or a sleep/wake reshuffle)
  silently stopped the audio with no banner and no recovery — only explicitly named
  devices got disconnect detection. The default now resolves to the same device through
  the path that watches for disconnects, so the usual "Sound card stopped — reopening"
  self-heal covers it too.

- **Pounce desktop notifications actually appear now — on the Mac for the first time.**
  The rare-DX alert used a browser notification API that Apple's webview simply does not
  have, and no platform ever asked the OS for permission, so the notification half of
  Pounce was silently dead everywhere. Alerts now go through the operating system's own
  notification center; macOS asks you once, at your first alert (never at launch), and if
  you decline, the alert sound and the in-app banner carry on exactly as before.

- **The waterfall's "move both markers" click works on the Mac.** Ctrl+click there is the
  system right-click gesture — it silently moved only the TX marker — and ⌘-click (the
  chord a mac WSJT-X operator's muscle memory sends) did nothing. Both now set RX and TX
  together, the on-screen hint reads ⌘ on a Mac, and Ctrl+click keeps working everywhere
  else exactly as before.

- **Memory quick-recall answers to ⌘1–9 on the Mac.** The recall chord was Ctrl-only —
  which macOS itself uses to switch desktops, so the press never even reached the app —
  and Cmd was explicitly rejected. Either modifier now recalls a favorite on every
  platform, and the chip tooltips advertise the platform's own chord.

- **F-key advice for Mac keyboards.** The CW and RTTY macro buttons, the voice keyer, the
  Operate Decode button and the WSJT-X switchers' guide advertise F-keys that a default
  Mac keyboard treats as media keys; on a Mac their hints now say to hold Fn (or enable
  standard function keys in System Settings). Nothing is rebound — the keys themselves
  are unchanged.

- **Every "open in your browser" link in the app now actually opens it.** The CAT-driver
  download link, the repeater directory credits, the contest-calendar rules links and APRS
  station pages were silently dead on every platform — the webview swallowed the click and
  nothing happened. They now route through the same mechanism the QRZ links always used,
  and ⌘-clicking a link works on the Mac too.

- **Field Day exports save a real file on macOS.** All four export buttons (Cabrillo,
  ADIF, summary, dupe sheet) went through a browser download path that macOS discards
  outright; they now write straight into your Downloads folder like the Logbook exports,
  and the confirmation names the saved path.

- **The Journey share card no longer claims success while doing nothing on macOS.**
  Copy-to-clipboard now uses the pattern Safari's engine accepts, and when the clipboard
  refuses, the card is saved as a PNG in Downloads with the message naming the file — a
  success message only ever reports a copy or a write that really happened.

- **A crash or force-quit can no longer strand rigctld/rotctld on macOS and Linux.**
  Windows has always killed the CAT and rotator daemons with the app; on Mac and Linux
  a crash, a Force Quit, or a hung shutdown could leave one running — holding your
  serial port open and sometimes making the next launch's CAT land on the stale
  daemon. Nexus now tracks every daemon it starts, stops any stragglers on quit, and
  each launch first cleans up anything a dead instance left behind (a second running
  Nexus and its daemons are recognized and left alone).

- **Installing an update on macOS and Linux now actually restarts Nexus.** The banner
  said "Nexus will restart…" but on those platforms the updater only swapped the app
  on disk and left the old build running — the banner hung there forever and nothing
  restarted (Windows was fine; its installer restarts by itself). The restart now
  happens, and it goes through the normal shutdown: transmitter unkeyed, logs and
  window position saved, never a hard kill.

- **⌘Q now quits Nexus properly on the Mac.** The menu quit (and its ⌘Q shortcut)
  bypassed the entire shutdown path: no wait for the transmitter to unkey, no
  conversation or Field Day flush, and no window-geometry save — a Mac operator who
  always quit with ⌘Q reopened at the default window box every launch, with the last
  15 seconds of chat at risk. Every quit route now runs the same cleanup closing the
  window does.

- **The docs caught up with macOS shipping.** The FAQ, README, install guide, manual
  and wiki no longer say "macOS does not ship" — the Mac build (signed, notarized
  Apple Silicon DMG, self-updating) has been out since 1.5.0. The install guide gains
  real macOS sections: drag-to-Applications install, upgrade, uninstall, where your
  data lives (`~/.config/tempo` — same place as Linux, on purpose), a `shasum`
  checksum example, and the one Mac-only gotcha: on first launch macOS asks for
  microphone access, and declining it silently kills all decoding — Troubleshooting's
  "No decodes" checklist now starts with **System Settings ▸ Privacy & Security ▸
  Microphone**. The old "put rigctld on PATH" advice for Mac is gone everywhere; the
  actual mechanism is `brew install hamlib` (Nexus searches the Homebrew/MacPorts
  prefixes itself — a Finder-launched app never sees your shell PATH).

- **CAT trouble on a Mac is now diagnosed in Mac terms — and a missing Hamlib is never
  blamed on your cable.** Three verdicts stopped guessing: Test CAT's baud ladder no
  longer buries the correct "Hamlib's tools aren't installed — brew install hamlib"
  diagnosis under a "close other CAT software" port guess (it was the probe tool itself
  that failed to start, and no port was ever touched); Auto-test says the same instead
  of "check the cable and that the rig is on" when its throwaway daemon can't spawn at
  all; and a rotator's rotctld failing to launch names the per-platform install cure
  instead of a raw "No such file or directory (os error 2)". The CI-V no-answer
  walkthroughs and the Settings port hints also stop talking Windows at Mac operators:
  /dev/cu.* examples and "the port list" instead of COM16 and "Windows Device Manager",
  the dual-port Icom tie-break in cu.* name order instead of a driver label that only
  exists on Windows, and no more advice to install a USB driver macOS already ships
  in-kernel. Messages also stop claiming a "bundled" rigctld — nothing is bundled on
  macOS or in the AppImage.

- **A Mac serial port saved as /dev/tty.\* now heals itself to its /dev/cu.\* twin.**
  Earlier Mac builds (1.5.0–1.6.1) listed every serial port twice and let you pick the
  /dev/tty.\* row — a node that hangs CAT on carrier detect instead of failing. The
  later picker fix only stopped NEW picks; a port already saved kept hanging after the
  upgrade. Nexus now substitutes the cu.\* twin wherever the stored name is consumed —
  the CAT daemon, native CI-V, PTT keying and Test CAT's baud ladder — and rewrites the
  saved setting once at launch when the twin is present, so the Settings screen shows
  the port that actually opens. A lone tty.\* with no cu.\* twin is left exactly as
  stored: it is the only node there is.

- **WSPR and FST4W beaconing works the way WSJT-X's does.** Four fixes from one
  report (#101): the TX watchdog no longer halts a beacon a few intervals into a
  session — beaconing is unattended repeated transmission by design, and WSJT-X
  exempts exactly these modes (each transmission is still hard-bounded by the period
  clock, and your TX switch remains the stop); a Round-Robin rotation of one station
  no longer transmits every interval — one slot is no rotation, so the transmit-%
  schedule applies and Settings says so; your own beacon transmissions now appear in
  ALL.TXT as Tx lines like every other mode's; and a WSPR spot ("CALL GRID DBM") now
  files in the operating roster under its callsign with its grid, instead of the grid
  showing up as a phantom station.
- **N3FJP general logging now carries your reports, the operator's name, and power.**
  The ACLog push was sending only the contest fields, so RST sent/received, name and
  TX power logged in Nexus never reached the ACLog side. The Field Day push is
  unchanged byte for byte — a contest exchange carries none of these. (#106)
- **Changing bands mid-period no longer lets the old band's last seconds decode into
  the new one.** Audio captured before a QSY was still decoded at the period boundary
  after it, so stations from the band you just left repopulated the roster the band
  change had rightly cleared. That period's decode is now dropped — the next full
  period on the new band decodes normally. Receive-side only. (#103)
- **The Log QSO button no longer claims success for a QSO it refused to log.** The engine
  deliberately refuses a manual log when there is nothing to write — the contact already
  logged, no active QSO, no report exchanged yet — but the green "Logged QSO" toast showed
  anyway, so a double-click after auto-log looked like a second entry that later seemed to
  vanish. A refusal now says so honestly: nothing was logged, and why. (#100)
- **The docked Band activity strip tunes on scroll, like the pop-out band map.** The strip
  is the same frequency scale the band map is, in the Phone and CW cockpits both, but a
  wheel over it did nothing while the map, the readout digits and the waterfall all tuned.
  It now rides the same wheel — your tuning step, your wheel sensitivity, and the same
  stop-at-the-band-edge behavior as every other dial. (#96)
- **macOS now says how to get Hamlib instead of quoting a Debian command.** A Mac operator
  whose IC-7300/IC-9700 wouldn't connect was told `sudo apt install libhamlib-utils` — a
  cure for the wrong operating system. On a Mac the message now says `brew install hamlib`,
  the FAQ covers macOS, and the in-app guide no longer claims Hamlib ships inside the
  installer on platforms where it doesn't. (WSJT-X and the Mac loggers link the Hamlib
  *library*; Nexus drives CAT through the `rigctld` *program*, a separate package — those
  apps working is not evidence rigctld is installed.)
- **The rig port picker on macOS no longer lists the tty twin of every port.** The #92
  collapse (1.6.0) filtered rig *detection* but not the Settings picker, so `/dev/tty.*`
  rows still sat beside their `/dev/cu.*` twins — and a picked `tty.*` hangs on carrier
  detect instead of failing. The picker now gets the same collapse; a lone `tty.*` with no
  `cu.*` twin is still kept.
- **Removing a radio: the button is always there now, and a refusal says why.** Nexus
  refuses to remove the active radio, but expressed that by not rendering the Remove button
  at all — from the operator's chair, "the radio won't delete." It now renders disabled
  with the rule spelled out: make another radio active first. A
  refused removal is a visible error instead of a silent success, and deleting the radio
  the rig form was editing no longer leaves later Saves silently writing to a profile that
  is gone.
- **The ATU button reaches the FT modes.** Phone, CW and SSTV have carried the rig's
  built-in-tuner button since it was added; the FT cockpit's TX cluster was missing it.
  Same rules as everywhere: it appears only when the rig reports a tuner, sits beside
  Tune, and every refusal is shown with its reason.
- **The signed macOS bundle carries the microphone entitlement.** Hardened-runtime apps
  need `com.apple.security.device.audio-input` before macOS will even offer the microphone
  permission prompt; without it, RX audio capture can open and deliver silence. The
  entitlement ships in the bundle now.

### Changed

- **The Xiegu and FlexRadio setup pages say what the app actually does.** The Xiegu page
  promised that Detect auto-matches an X6100/X6200 — it never could, because those radios
  report their USB bridge chip and not a model name — and never mentioned the two serial
  ports they present or the 19200 baud the family runs at. The FlexRadio page told you to
  launch a second copy of Nexus for a second slice, which is not how multi-radio works
  (add a second radio, tick "Run both radios at the same time", one window each), and
  named a PowerSDR model string the dropdown does not contain. Both are rewritten from the
  code, and the Flex page gains a section on the two *early access* native toggles: off by
  default, never run against a real Flex, same-LAN only (they cannot work over SmartLink
  or through NAT), and native DAX takes over the radio's transmit audio for every client
  while it is on.

- **macOS: the Local Network permission is now documented where it bites.** A FlexRadio,
  or any network rig on the LAN, is reached through a permission macOS 15 gates and Nexus
  does not yet have a usage string for — so there may be no prompt, and a denial is silent
  rather than an error: "No radios found", or CAT reporting that nothing answered. The
  install, troubleshooting and FlexRadio pages now describe that and point at System
  Settings ▸ Privacy & Security ▸ Local Network.

## [1.6.1] — 2026-08-17

### Fixed

- **The waterfall can no longer freeze and pretend to be live.** Field reports the day after
  1.6.0: waterfalls stopping after seconds or minutes. Four defenses landed, one per route
  into the symptom: a stuck transmit-hold now times out after six minutes instead of freezing
  the display forever; the TX dark band no longer applies to RTTY and SSTV, whose minutes-long
  overs made it read as a dead display (it remains on FT8, where it belongs); a wedged row
  fetch is abandoned after five seconds instead of silently ending all display updates for
  the session — on both the waterfall and the Phone/CW scope; and after an audio-device
  rebuild, the error banner now stays up until the new device actually delivers samples,
  so a recovery that silently failed can no longer clear its own warning.
- **Windows: USB rig interfaces work again- **Windows: USB rig interfaces work again — the DE-19/QDX audio regression from 1.3.0.**
  Reported with a clean regression window (#99, Xiegu DE-19; #104, QRP Labs QDX): audio
  failed to open with "the requested stream type is not supported." A 1.3.0 Linux fix taught
  the audio open to share one device handle when the input and output names match — right on
  Linux, where a device is the whole sound card, and wrong on Windows, where a device is a
  one-direction endpoint and a USB rig interface carries the same name on both. Nexus was
  handing the output stream a capture endpoint. Sharing is now platform-aware, and even where
  sharing is believed correct, a handle that can't produce an output config falls back to
  real resolution instead of failing the open. Until you have this fix: renaming the rig's
  Playback device in Windows Sound settings works around it.

## [1.6.0]## [1.6.0] — 2026-08-16

### Fixed

- **FT2 answers in the next slot, not three later.** The first on-air QSOs exposed it: FT2
  keyed each over before the previous slot's decode had folded in, so every reply slipped a
  full cycle and every message went out twice — both stations reading each other as "didn't
  copy". FT2 now decodes the completed signal 1.15 s before the boundary and keys with fresh
  state, exactly like FT4. Its transmission also now starts at the slot boundary, matching
  the Decodium convention on the air, and the band roster no longer ages stations out four
  times too fast on short slots.
- **The Phone and CW scope window follows your radio's filter.** A 500 Hz CW filter inside
  the old fixed 800 Hz window left dead margins that could never light — the window now spans
  the filter (plus a little skirt), and Phone's default span is a new Auto preset that tracks
  the rig's bandwidth.

- **A satellite over can no longer be yanked onto the downlink by a stale keyed-state poll.**
  Proven on the wire from the operator's CI-V capture: the mic went down, the once-a-second
  PTT poll hadn't noticed yet, and the dial-keep pushed the downlink onto what was now the
  transmit VFO. The rig's own reported frequency is now the evidence — if it reads the
  pass's uplink, the rig is treated as keyed no matter what the last poll said — and the
  PTT poll runs five times a second while a pass is up. Works for linear birds too, and a
  bird's other documented uplink pairing (145.200 vs 144.490 on the ISS) counts as the pass,
  not as you turning the knob. The pass also pins its transponder row at AOS, ending the
  first-correction flip between rows the same capture caught.
- **The Phone scope gives the voice three quarters of the panel.** The centered axis was
  symmetric, so the (empty, filtered-away) far side of the dial cost half the display and
  speech looked compressed into one section. The dial line now sits at the quarter mark —
  left of center on USB, right on LSB — and the occupied sideband gets the rest.

- **The setup wizard's example callsign is nobody's callsign.** The callsign and grid boxes
  showed `KD9TAW` and `EN52` as hint text on a fresh install — readable as prefilled values,
  and they are a real station's. The examples are now `N0CALL` and `FN31` everywhere hint
  text names a callsign or grid.
- **The Phone and CW scope centers your frequency, the way your rig's scope does.** Reported
  from the bench: a station on your own frequency painted at the left edge of the passband.
  The audio-fed scope's axis started at the dial and ran upward, so the dial sat on the far
  left pixel with nothing marking it. The Phone scope now puts the **dial at the center** with
  a labelled line — voice extends to the right on USB, to the left on LSB, exactly as a rig
  draws it — and the CW scope centers on **your sidetone pitch**, so a zero-beat station and
  the pitch hairline sit mid-window. The quiet half of the Phone display is honest: on the
  soundcard feed the radio filtered that side away before Nexus ever heard it; rigs with a
  native panadapter feed (Flex, CI-V scope) show real signal on both sides as before.

- **Satellite passes stop losing their split half a second into an over.** Found on the air
  (IC-9700, ISS V/V): a fast dial-read during a keyed over saw the transmit VFO's frequency,
  read it as you turning the knob, and tore the split down — the rig dropped to simplex on
  the downlink and keyed there. Three holes closed: the fast read and the dial-keep write now
  wait while the rig is keyed, and an FM bird's own two frequencies are recognised as the
  pass (within Doppler) instead of being structurally unrecognisable, which is what made
  every FM satellite take the teardown path. Doppler keeps correcting the uplink through the
  over. **Needs on-air verification.**
- **The uplink mode is now stated, not assumed.** An FM bird's transmit VFO kept whatever
  mode the previous pass left on it — this morning that was an inverting bird's LSB on
  145.990. Nexus now commands the uplink mode whenever it owns the transmit leg (once per
  pass, not per tick), and the write reaches the Icom's unselected VFO directly so it lands
  on the register the rig actually transmits from. Same fix covers non-inverting linear birds.
- **A same-band (V/V) pass takes the 9700 out of satellite mode first, and puts it back.**
  The rig's satellite mode is crossband-only, so a V/V pass fought it and lost — Nexus now
  probes the front-panel state, steps it aside for the pass, restores it after, and says so
  in the CAT detail. Only a state Nexus changed is ever restored.
- **FM uplinks carry their CTCSS tone.** The ISS repeater needs 67.0 Hz and Nexus was
  actively zeroing it on satellite passes — the repeater could never open. Known FM birds
  (ISS, SO-50, AO-91, PO-101) now key their published tone; the tone ends with the pass.

### Added

- **Text that carries your callsign IS your SSTV ident.** Put your call in any text overlay
  and the corner ID plate retires — your layout identifies the picture, not the app's
  stamp. Delete that text and the plate returns on the same draw. The manual "already in
  the picture" checkbox still works as before.

- **Every waterfall can now be hidden.** The scope strip in Phone, CW, RTTY, and SSTV joins
  the ⊞ Panels menu (Operate's already lived there) — on by default, and a hide sticks until
  you bring it back. The panes below take the freed height.

- **FT2.** The fast slotted mode from the Decodium community (IU8LMC's WSJT-X fork) joins the
  FT dropdown: FT4 with a halved symbol time — 3.75 s periods, ~167 Hz wide, decoding to
  −10.8 dB — for when the band turnover is worth more than the last few dB. Nexus follows
  Decodium's own band plan (a few kHz above each FT8 hole, 160 m through 23 cm), runs the
  same auto-sequencer as FT8/FT4, shows a-priori decodes with the same `a` marker, and logs
  as MFSK/FT2 so LoTW and the online logbooks accept the record. Built from Decodium's own
  GPL modem sources (see NOTICE) and proven against its published sensitivity in the test
  bench — **not yet validated on the air against a live Decodium station.**

- **Text on your SSTV pictures.** The composer can now lay text over the picture before
  transmitting, the way MMSSTV does. One-click **CQ**, **73** and **Reply** cards — Reply fills
  in the other station's callsign from the newest FSK ID heard — plus free text. Two styles:
  **Crisp**, the same pixel font as the burned-in ident, proven readable through the decoder;
  and **Banner**, big display text with a thick outline. Eight colours, four sizes, and every
  item carries a solid plate or an outline — that contrast edge is what keeps coloured text
  readable on the far end. Drag text where you want it (or use the arrow keys); your station
  ID always draws last, so nothing you add can cover it.
- **Edit & resend from the SSTV gallery.** The pencil on a received picture loads it into the
  composer — answer a station's picture with their picture and your text on it.
- **Hunt a grid square by name.** The watch list (Settings ▸ Spots & Alerts) now takes grid
  squares alongside callsigns and DXCC entities: enter `FN31` or `EM7*` and get the loud ⭐
  alert the moment a station decodes from there, with the Work button ready. A square you ask
  for by name alerts on every band — the HF grid-quiet default doesn't apply to it.

## [1.5.0] — 2026-08-16

### Added

- **Nexus runs on the Mac.** A native Apple Silicon build (M-series, macOS 12+), signed and
  notarized, with the same self-updater as Windows and Linux. CAT control uses Hamlib from
  Homebrew (`brew install hamlib`); Intel Macs can build from source, which works out of the
  box as of 1.5.0. First macOS release — treat it as fresh ground and report what you find.
  *(Added to the 1.5.0 release 2026-08-16, after the initial publish — same source, new platform.)*

- **MSK144 has a display built for meteor scatter.** Switch to MSK144 and the waterfall strip
  becomes a time display — the Fast Graph, as WSJT-X draws it: seconds across one T/R period,
  a green power trace where a ping is a spike you can see land, the current period above the
  previous one, and a marker with the callsign at each decode. On meteor scatter every signal
  sits at 1500 Hz and lives for milliseconds, so a frequency waterfall showed one unmoving
  stripe while the actual event was invisible. A 5/10/15/30 s T/R selector sits beside the
  mode pills — on meteor scatter the period is an operating decision, not configuration.
- **MSK144 pings appear while the period is still running.** Nexus used to decode once at the
  period boundary, so you watched a silent screen for 15 seconds and then got history. Pings
  now reach the decode list about two seconds after they land, mid-period, with the T column
  showing when in the period each one arrived.
- **Worked-before comes in two strengths.** The B4 chip is hollow when you have worked the
  callsign anywhere and solid when you have worked them on the band you are on now — the same
  two scopes WSJT-X colours separately. A new setting (Logging ▸ Worked-before (B4) & dupes ▸
  **Match mode too**, off by default like WSJT-X's) makes 40m FT8 and 40m phone count as
  separate contacts for both the chips and the log strip's Dupe badge.
- **The Spots panel shows the state it already filtered by**, as a sortable column, and the
  Phone/CW band-strip flags carry the state in their tooltips.
- **Program can take channels you type.** *Add by hand* enters a repeater or simplex channel
  the directory has wrong or missing, and *Import CHIRP CSV* reads the same format Program
  exports and CHIRP itself saves. Memories has had both for a while; now the workbench that
  builds your radio's channel list does too.
- **The rig scope's waterfall gets the scroll-direction button**, matching the FT8 waterfall,
  and defaults the same way (newest at the top). RTTY and SSTV share the FT8 waterfall and
  already had it.

### Fixed

- **A dead sound card recovers instead of failing silently and permanently** — contributed by
  on8st. A capture stream error used to be logged once and never acted on: the audio thread
  kept running against a card that would never deliver another sample, so the waterfall froze
  until a restart. A stream error now puts the card on probation, silence confirms the death,
  and the card is rebuilt — waiting for your key to come up first, so a recovery never cuts an
  over. A card that flaps is rebuilt at a steady rate rather than the flap rate, and a machine
  whose default device disappears entirely keeps retrying instead of giving up for the
  session. (#73, #74)
- **Adding a radio no longer switches the station onto it** — contributed by on8st. Pressing
  "Add radio" silently moved the station onto the new, empty profile — tearing down the
  working rig's CAT to bring up one with no port and no model, which froze the interface and
  blanked the settings pane. A new roster entry is just a roster entry now; the form opens on
  it for editing, and "Make active" stays the deliberate act it always was. (#91)
- **Every serial port showed twice on macOS** — contributed by on8st. The system publishes
  each USB-serial adapter as a tty/cu pair, and the rig picker listed both; the tty twin
  could hang a CAT probe waiting for a carrier-detect line no radio asserts. Only the cu
  entry is listed now. (#92)
- **A rigctld that cannot actually run no longer wins the CAT probe** — contributed by on8st.
  A Hamlib built from source can be first on PATH yet die before main with an unloadable
  library; Nexus committed to it and CAT came up dead with no usable diagnosis. Every
  candidate is now spawn-checked first, and one that cannot run is skipped for one that can.
  (#70)
- **Nexus builds on a Mac without hand-set linker paths** — contributed by on8st. The
  gfortran and FFTW3f runtimes live under Homebrew's prefix, which Apple's linker does not
  search; the build now asks the toolchain where its own libraries are. With the golden
  fixture made portable across architectures (#90), the test suite passes on Apple Silicon
  out of the box. (#69, #90)
- **Working the ISS voice repeater no longer transmits on the bird's downlink.** Two faults,
  both found on the air. A same-band (V/V) pass asked a Main/Sub mapping to do something those
  radios cannot — Main and Sub can't both sit on 2 m — so nothing was written and TX stayed on
  the downlink; a V/V pass now rides the A/B split, which is how those passes are worked. And
  a rig keyed from the HAND MIC didn't count as transmitting, so half a second into an over
  the Doppler tick and the dial-keep would retune the transmit VFO back onto the downlink. A
  mic-keyed rig is now keyed everywhere.
- **"Call CQ" stops claiming you are calling CQ.** The CQ/S&P toggle lit Call CQ through every
  S&P contact and forever after, because it read a flag a directed call also sets — and the
  AUTO-CQ pill made the same false claim. Both now read the real CQ-run state, and the idle
  Call CQ button drops its permanent red border: in this app red means TX-armed, which an idle
  station is not.
- **Your own transmission scrolls as a dark band on the waterfall** — the honest "not
  listening" gap, as WSJT-X and your rig's own scope draw it. It used to paint a full-width
  red band after every over: while keyed the rig's muted receiver dragged the display's
  auto-contrast down to digital silence, and key-up clamped the whole band to the palette's
  hot end for a couple of seconds. The false rows are dropped at the source now and the
  contrast holds through the over.
- **CONFIRM is now NEEDS QSL, and says what it means.** The tag marks an entity, zone or grid
  worked on this band but not yet confirmed — a fact about your award slot, never about the
  callsign in the row. The old "Worked —" wording read as a claim about the station and made
  NEEDS-QSL-without-B4 look like a bug; it is the normal case.
- **CW keyed over CAT sends the whole macro.** Every one-character word — the closing K, a
  bare ? — was silently rejected by the keying library, which reads a single-character message
  on a Yaesu as a stored-memory number; a lone digit 1–5 would even have played the rig's
  stored message instead of keying the digit. Padded past the trap. (#86)
- **Ham Radio Deluxe forwarding actually forwards when it is the only connector enabled.** An
  internal gate skipped the send loop unless some other upload service was also on, so an
  HRD-only station queued contacts forever and sent none — while the app said "Logged". (#87)
- **The update banner's "Not now" sticks.** Dismissing an update lasted until the hourly
  re-check, which re-downloaded it and put the banner straight back. A dismissal now holds for
  that version; a newer release still lands.
- **Re-importing a log with PSK31 respelled as BPSK31 no longer doubles those contacts** — the
  last spelling pair from the import-dedup work that shipped in 1.4.0. (#31)
- **The "check for updates" tooltip stops naming SourceForge** — the check reads the project
  site first, and the Download button opens GitHub Releases.
- **Entering MSK144 parks both frequency offsets on 1500 Hz**, where the mode actually
  transmits — the red TX marker could sit at 2400 Hz while the rig keyed 1500 — and the DT
  column reads as T (time of the ping within the period) instead of painting every healthy
  ping red with FT8's clock-skew colouring.

### Changed

- **HF is grid-quiet by default.** The rare-grid (💎) alerts now scope to VHF and up like the
  plain grid alerts — on HF nearly every decode is an unworked grid, so even the rare tier
  read as chatter. An HF grid-chaser widens it back in Settings ▸ Spots & Alerts.
- **The FT8 cockpit no longer hosts the memory favorites strip.** Memories are repeaters,
  nets and calling frequencies — Phone and CW things, and those cockpits keep the strip. In
  the FT8 header a favorite chip was one click from retuning the rig off the band
  mid-sequence. The recall hotkeys work everywhere, unchanged.

## [1.4.0] — 2026-08-15

### Fixed

- **A radio that refuses the filter width no longer strands you on its widest one.** Reported on
  a Flex 6400: the filter ends up at 6000 Hz after a mode or band change. In the data modes Nexus
  sends the mode and a 3 kHz width in one command, so a radio recalling a narrow DATA filter
  cannot clip FT8. If that command keeps being refused, Nexus retries a while and then sends the
  mode with the width left to the radio's own default — otherwise a radio that objects only to
  the width would end up with no mode set at all. On a Flex that default is the full 6 kHz SSB
  filter, which is not a filter anyone wants for FT8, and nothing on screen said where it came
  from. Nexus now asserts the 3 kHz again the moment the mode is in; if the radio refuses the
  width as well, the CAT status says so and tells you to set the DATA filter on the radio, rather
  than leaving you with a 6 kHz filter and no explanation. (#82)
- **Signal reports are logged in the form they went out on the air.** Reported by an operator
  running Log4OM: contacts arrived in Log4OM complete except for the sent and received reports,
  which were blank. Nexus transmits `-07` and `+03` — the two-digit signed form every FT mode
  puts in the message, and the form WSJT-X writes to its log — but recorded them as `-7` and,
  worse, `3`, dropping the plus sign entirely. A report with no sign is not a signal report a
  logging program can read, which is why the field came through empty. The report Nexus logs is
  now byte-for-byte the report it transmitted, so it reads correctly in Log4OM and any other
  logger on the WSJT-X link, and in your own logbook, ADIF exports and LoTW and QRZ uploads.
  Contacts logged before this keep whatever they were written with; re-exporting does not change
  them.
- **The AGC Fast/Mid/Slow buttons reach the radio every time you press one.** Reported from CW:
  "AGC changes for F-M-S work slowly or not at all." Two things were wrong. Nexus only sent an
  AGC command when the speed differed from the one it had last sent — but your radio's AGC moves
  without Nexus (the front-panel knob does it, and so does the radio recalling its own per-mode
  AGC when Nexus puts it into CW), so once the two disagreed, pressing the speed Nexus thought
  it had already set sent nothing at all, for the rest of the session. Pressing a button is now
  always a command. And if the radio refuses a step — Hamlib's AGC is a fixed list and not every
  radio has every entry, Mid least of all — Nexus used to re-send that rejected command every
  20 ms forever, which is what made everything else on the CAT link feel sluggish. It now stops,
  says so, and the highlighted button drops back to the speed the radio is actually on instead
  of claiming one it never took.
- **Push to Talk tells you when transmit is switched off instead of doing nothing.** Reported on
  an FTdx10: clicking Push to Talk did not key the rig, and nothing on screen said why. Nexus
  drops a mic key for two different reasons — you are outside your licence privileges, or
  transmit is simply switched off — and the button only ever showed the first. With transmit off
  (Stop TX, the transmit watchdog, or a logger sending Halt Tx all switch it off) the button
  still read PUSH TO TALK, and pressing it turned the button red and said ON AIR over a rig that
  was not keyed. From the operator's chair that is indistinguishable from a bad PTT cable. The
  button now reads **■ TX OFF — CLICK TO ENABLE**, does not pretend to be on the air, and that
  click turns transmit back on — press it again and you talk. The Phone screen had no other
  transmit-enable control on it, so the only way out used to be leaving the screen and coming
  back. The voice keyer's message says the same thing now. (#81)
- **Hints in text boxes stop looking like settings you already entered.** Reported on dark mode:
  the grey example text in an empty box — `127.0.0.1:5002`, `COM16`, `Radio name` — rendered at
  nearly the brightness of a real entry, so a Settings page full of empty fields read as a page
  full of configured ones. Every text box in the app now draws its hint in the same faint ink
  the rest of the interface uses for secondary text, in all four themes, so an empty field looks
  empty at a glance.
- **Your RX filter stays where you put it when you change frequency.** In the data modes Nexus
  sends the mode and a 3 kHz width together, so a rig that recalls a narrow DATA filter cannot
  clip FT8. It was sending that width on *every* frequency change — so each spot click, Needed
  pick or in-band QSY reset the filter and popped the rig's Width display, undoing anything you
  had set by hand. Now the width only goes out when the mode or the band actually changed, which
  is when the rig might have recalled a filter of its own. Move around inside a band and Nexus
  leaves the filter alone. (#67)
- **Changing the operator no longer drops you out of the QSO you are in.** Handing the key to
  the next operator — the chip in the top bar, the Field Day panel's Operator field, or the
  torn-off scoreboard — saved that one name by writing the whole settings file back, which
  resets the app the way pressing Save in Settings does: back to Chat, anything queued to send
  thrown away, and the transmit cycle reset to whatever it was when the panel was opened, so
  the next over went out in the same period as the station you were working. Nothing on screen
  said any of it had happened. A seat swap now writes only the operator's name; the contact in
  progress, the queue and your cycle are left exactly as they were. (#54)
- **4 m is in the SSB and CW band pickers now, not only in FT8.** The FT dropdown has carried
  4 m for a while — 70.154 for FT8, plus the JT65, MSK144 and WSPR dials — but the Phone and CW
  cockpits read a different band list, and that one had never heard of the band. So you could
  work 4 m FT8 and then watch the band vanish from the dropdown the moment you switched to SSB
  or CW. Both pickers offer it now: a phone pick parks on 70.100 and a CW pick on 70.200, the
  Region 1 SSB/CW calling frequency, rather than the dead 70.000 band edge. 4 m stays a Region 1
  band — there is no US allocation at any class, so it appears for the non-US (Open) class and
  stays off a US-licensed operator's dropdown, exactly as the transmit lockout has always
  treated it. National 4 m edges differ by tens of kHz inside 70.0–70.5; check yours before you
  key.
- **Opening POTA / SOTA no longer puts your radio into DATA.** Reported on an FT-991A and seen
  again on a Flex 6400: clicking POTA / SOTA in the sidebar flipped the rig from SSB into
  DATA-USB, on every band, since 1.0.0. The hunting board declares a workspace so it can borrow
  the wide layout, and Nexus was reading "has a workspace" as "is an operating mode", then
  falling back to digital for anything it did not recognise. A hunting board is not a mode — you
  work a park on whatever the activator is running — so it no longer touches the radio at all.
  Clicking HUNT still sets the frequency and mode of the spot you picked, which is the moment
  that should move your rig. (#80)
- **Signals rise and fall on the CW and Phone scope again.** Reported from an FTdx10: "I see big
  vertical spikes where the voice is; on Nexus it seems like it's all smoothed out without the
  aggressive peaks, the whole spectrum is up with minimal rises and falls." Two separate faults
  made that picture. The scope set its top of scale from the loudest thing on screen, refitted
  every row, so a signal 40 dB out of the noise drew at exactly the same height as one 12 dB
  out — a signal could not get taller because it was already at the top. And it set the bottom
  of scale from the quietest bins in view, which on an audio scope are the far side of the
  radio's own filter, 40-odd dB below the band noise — so the noise floor itself was drawn near
  the top of the panel with nothing left to rise above it. The scope now works the way a rig's
  does: the noise sits at the bottom, the scale above it is fixed, and how tall a signal draws
  is how strong it is. The readout beside the palette controls now shows how far the strongest
  signal stands above the noise, which is the number worth watching.
- **A carrier is a line on the scope, not a block.** Every scope in Nexus assigned the analysed
  frequencies to display columns in a way that let neighbouring columns claim the same data, so
  one carrier was painted into two or three columns at identical height — a flat-topped block
  before anything was drawn. Carriers now land in one column with real shoulders either side.
  The CW and Phone scope also asks for its detail across the span it is actually showing rather
  than the whole receiver passband: on the CW cockpit's 800 Hz window that is five times finer,
  for the same amount of data, so a signal is drawn as a shape instead of a spike stretched
  across seventy pixels.
- **CW keying is visible on the scope.** The trace held each peak for four tenths of a second
  before letting it fall, which is right for speech and far too long for CW — at 25 WPM the gap
  between two dits gave back a ninth of the height, so keying drew a solid bar. The CW cockpit
  now uses a hold short enough to show the rhythm, and Phone keeps the longer one that stops a
  voice flickering between syllables.
- **The scope's paused scrollback can be zoomed out.** Pausing the CW or Phone scope lets you
  wheel back through the band, but the stored history only ever covered the window that was on
  screen when each line arrived — everything either side of it was discarded as it came in, so
  widening the view while paused found nothing there. The scope now keeps the full width of what
  it received, so scrollback can be widened after the fact.
- **CW contacts carry the other station's grid square into your log.** Reported from CW: contacts
  logged with the grid field empty. The grid is not a box you can type on the CW or Phone log
  strip — it is filled in behind the scenes from the callbook — and that lookup only ever ran for
  a callsign you had typed by hand. In CW you almost never type one: the decoder fills the call
  in for you and the cursor jumps to the report. So the lookup never ran and every CW contact
  logged without a grid, a state or a country. A call that arrives from the decoder or from
  clicking a spot is now looked up like any other, once it is settled. The grid still depends on
  your callbook account returning one.
- **A DXpedition worked with a bracketed callsign logs as itself.** Reported by an operator
  working a DXpedition on FT8. When a callsign will not fit in the message, the FT8 protocol
  sends it wrapped in angle brackets, and Nexus was writing those brackets into the log. The
  contact then read as a different station from every other one with that callsign, and it
  counted for nothing on the awards board — the country lookup cannot match a callsign with
  brackets in it, so a new country and a new band both came back false, silently. The brackets
  are now removed where the contact is written and where it is scored, and left alone on the
  air, where the protocol requires them. (#84)
- **A station repeating their report gets answered instead of ignored.** Reported on FT8:
  contacts closing early, with the other station still calling. When you have sent your final
  roger and the other operator did not copy it, they send their report again — and Nexus had no
  answer for that, so it went quiet at the exact moment they were asking it to speak, and moved
  on. It now sends the roger again, which is what WSJT-X does in the same spot. (#59)
- **A radio that keys the moment Nexus starts can be told why.** Reported on a Kenwood TS-2000
  through a Digirig: the rig goes into transmit as the app launches, whatever the PTT setting
  is. A one-cable interface keys the radio from the same serial line the CAT commands travel on,
  and that line has to be held down or the radio reads it as a mic key. Nexus can only hold it
  down for cables it recognises, and a Digirig reports itself to Windows with exactly the same
  identity as several radios do — so it cannot be told apart from a rig that needs that line for
  its own handshake, and guessing wrong the other way takes CAT away from Yaesu owners entirely.
  So Settings ▸ Radio ▸ Rig & CAT now has **Interface keys RTS on the CAT port**. Tick it if your
  radio transmits as soon as Nexus starts, and it will stop. Leave it alone otherwise — the
  default is exactly the behaviour you have today. (#44)

### Added

- **A resolution control on the CW and Phone scope.** A button beside the palette controls that
  steps between three analysis widths, labelled by how sharply the scope can separate two
  signals: **23 Hz** is what Nexus has always used and stays the default, **12 Hz** halves the
  width of a carrier for picking a weak one out of a crowded passband, and **47 Hz** goes the
  other way. The last is worth knowing about if you work CW: at the default, the scope looks at
  a longer slice of audio than a dit lasts at 25 WPM, so keying cannot be seen as keying no
  matter how good the display is. On 47 Hz it can. Sharper costs response and faster costs
  detail — there is no setting that is best at both, which is why it is a button rather than a
  decision made for you. It highlights when you are off the default, and each scope remembers
  its own.
- **An ATU button that runs your radio's own antenna tuner.** Asked for in discussion #19: Tune
  keys a steady carrier, which is what you want for setting drive or tuning an external ATU, but
  it does nothing for the tuner built into the radio. There is now an **ATU** button beside Tune
  in the Phone, CW and SSTV headers that tells the radio to run its own tune-up, the way a
  right-click on Tune does in WSJT-X. It appears only on radios that report having a tuner over
  CAT — if your rig has none, there is no button to press and no mystery about why nothing
  happened. Because a tune-up makes the radio transmit, the button answers to the same rules as
  every other transmit control in Nexus: transmit has to be on, the dial has to be inside your
  licence privileges, and nothing else may be keying. When one of those is not true, Nexus says
  which, instead of quietly doing nothing. **Not yet tested against a radio** — the tuner command
  is Hamlib's standard one, and what each brand does with it is worth a report either way.
- **The Call Roster shows who each station is calling, and what state or province they are in.**
  Two more columns on the FT8/FT4 roster, sortable like the rest, as GridTracker has them.
  **Calling** is the callsign from that station's last decode — who they are working right now —
  or **CQ** when they are calling nobody, which is the row worth double-clicking. **State** is
  the US state, read from the FCC callsign index or from the grid they sent when the index has
  no record for them, and now the Canadian province too, read from the region number in the
  call: VE3 is Ontario, VY2 is Prince Edward Island, and so on for all thirteen. It shows as a
  pill, like the other badges on the row, so it reads at a glance down a busy column. It is the
  same answer the Needed board uses to decide "new state", so the two can never disagree with
  each other. Neither column is a callbook lookup: both come off decodes you have already heard,
  so there is nothing to set up and nothing leaves the machine.
  A Canadian station whose grid square straddles the border is no longer labelled with the state
  on the American side of it — the call says which country, and the call is asked first. Because
  the province is also the right value for the ADIF **STATE** field, Canadian contacts now carry
  it into your log and your exports.
- **The waterfall runs downward now, and you can put it back.** A **Scrolls up / Scrolls
  down** button in the waterfall header. Scrolls down is the new default — the newest line
  lands at the top and history slides downward, the way most rigs and a lot of other software
  draw it — and Scrolls up is what Nexus did before, with the newest line arriving at the
  bottom and the picture climbing. If you preferred the old direction it is one click away and
  it stays clicked. The switch repaints the history you have already collected, so nothing tears, and
  the paused scrollback wheel follows the same direction. Each waterfall remembers its own
  setting, so a torn-off waterfall on a second monitor can run the opposite way to the docked
  one. The 3D stacked view keeps its own front-to-back perspective either way.

## [1.3.0] — 2026-08-13

### Added

- **RTTY has a TX button now, like MMTTY — key up once and type.** The RTTY dock gained a **TX**
  button. Click it and the transmitter comes up and stays up: type and the characters go out as
  you type them, and between keystrokes the air carries diddle (the LTRS idle every RTTY station
  sends), so the far end holds sync instead of hearing you drop out at the end of every line.
  Click TX again and it sends the rest of what you typed and unkeys. No more pressing Enter for
  every line, which is what this was reported for. The F-key macros type into the live
  transmission while it is up rather than queueing a separate over behind it, and Enter puts a
  new line on the air instead of sending. Enter-per-line still works exactly as before when the
  TX button is off — nothing about the old way changed.

  Because this is the one transmission in Nexus that keys with no fixed end, it is wrapped in
  more stops than anything else, not fewer. Stop TX, the dock's Esc/Stop macro, the TX-enable
  switch and **Esc** (RTTY now has the keyboard shortcut CW always had) each cut it instantly.
  So does anything that takes RTTY off the rig without you pressing a stop at all — leaving the
  RTTY section, tuning to a frequency you are not licensed for, starting a tune carrier, or
  switching radios — each of which unkeys within about a fiftieth of a second. Your TX watchdog
  applies exactly as it does to any other over (typing keeps it happy, walking away does not),
  and above it sits a hard ten-minute ceiling on a single continuous over that nothing can
  extend, so a stuck key cannot buy an unattended carrier. If the app itself locks up, the
  transmitter unkeys on its own within about a second rather than staying keyed. Continuous TX
  and the auto-sequencer will not run at the same time, and each says so if you try.

- **A beam heading next to the country, everywhere the country appears.** Band Activity, the
  Call Roster, the Tempo roster, the Needed board, Spots, the chase panes, the pounce banner,
  the selected-station card, the DXpedition views and the map's tooltips all print the short-path
  bearing from your grid straight after the entity name — `Fed. Rep. of Germany 47°` — which is
  where every other logger puts it and where it was asked for. When the station sent a grid the
  heading is measured to that square. When it did not, it is measured to the middle of its DXCC
  entity and marked with a tilde, `~47°`, because the middle of a continental country is nowhere
  near most of the stations in it: every US callsign resolves to one point in Missouri, so a `~`
  heading is a hint about which way to turn, not a number to trust a beam to. Hover it and it
  says so, and says "short path", which is what all of these are. If your grid is not set in
  Settings, or cty.dat has no location for the country, nothing is shown at all — an honest gap
  rather than a confident 0° pointing due north. The roster's existing Brg column, which used to
  read "—" for any station heard outside a CQ, now fills in the same way and sorts on it.

- **Find a setting by typing what you call it.** Settings has a search box in its header, and it
  searches the words on the control rather than only the headings above them — "COM port",
  "sound card", "PL tone", "WPM", "keps", "stop bits", "no RF" all land on the right page even
  though none of those phrases is a heading. Picking a result takes you there, and if the
  setting lives inside a collapsed **Advanced** group it opens the group with the control in
  view, so you arrive looking at the thing rather than at the heading above it.
- **Every "Settings ▸ …" pointer in the app now takes you there.** The app named a Settings path
  in about 228 places and none of them were clickable — including several that named tabs which
  no longer existed. Setup health's own "no RX audio" light said to check the audio device
  *below* and then left you to find it. Those are buttons now. The rotator's "not answering"
  chip, for one, now opens the rotator settings — nearly always a wrong model or port — instead
  of naming a location and leaving you to hunt for it.
- **Nexus can hand your log to TQSL on a timer.** Settings ▸ Confirmations ▸ LoTW gained
  **Upload to LoTW automatically** — every few hours it runs the same batch the Logbook's
  "Upload to LoTW" button runs, signed by your own TQSL. It is off until you turn it on, and
  it is deliberately not a per-QSO push like the other services: LoTW takes one signed batch
  and gives back one result for all of it. Two things it will not do. It refuses to run at
  all while **Sign from ADIF location** is on — that mode signs the whole batch from wherever
  you are *now*, so a timer that fired after you moved would sign last week's contacts with
  this week's grid, and there is no undoing that at ARRL. And if a batch is ever refused —
  no certificate, wrong Station Location, one bad record — it stops and says so once, rather
  than re-signing and re-sending the same batch every six hours forever. Save any LoTW
  setting to start it again.

- **You can now tune off the ham bands from the app, and receiving there works properly.** Two
  things were wrong once you got off-band, and both are fixed. First, you could not get there:
  the wheel, the scope click, the ◄/► buttons and typing a frequency all refused anything
  outside the band plan, some with an "outside the band plan" error and one silently — so WWV or
  a shortwave broadcaster could only be reached by turning the knob on the radio. They all tune
  anywhere now. A fast wheel flick still stops at the band edge so you cannot leave a band by
  accident, and it tells you where it stopped instead of calling it an error; one more scroll
  goes past. Second, the parts of the app that ask "is this station new *on this band*" stay
  quiet while there is no band to ask about, rather than treating the empty band as one nobody
  had ever worked. The all-time questions still answer, so an entity you have never worked
  anywhere is still flagged as new wherever you hear it. The Band Activity pane says "off the
  band plan" instead of going blank, the decode pane no longer clears itself when you tune back
  to the band you came from, and the frequency readout only turns TX-red when your licence
  actually says so, rather than whenever the dial leaves the band plan.

  Transmitting is deliberately unchanged. Nexus does not refuse to transmit merely because a
  frequency is off *its* band table — that table is written around the US allocations, and the
  table is not the law everywhere. The UK 60 m allocation starts at 5.2585 MHz, below the 5.3
  the table calls the bottom of 60 m, so a blanket refusal would have blocked operators working
  their own legal frequencies. If you have told Nexus your US licence class it still fails
  closed off-band, exactly as before; if you have not, it trusts you, exactly as before. Leaving
  the bands still cuts a transmission that is in progress.
- **SSTV can be told a custom frequency is FM.** The USB/FM pick is now in the SSTV cockpit's
  band control on 10 m and up, so a local FM repeater or simplex channel you type in yourself
  can be worked like the built-in ones. It is deliberately not offered below 29 MHz, where the
  app does not command FM at all.
- **One Units setting — metric or imperial, everywhere.** Distances, temperature and wind
  speed now follow a single Units choice (Settings ▸ Station): automatic from your system's
  region, or metric/imperial by hand. It covers the spots that were still imperial — the
  station recall card, the roster distance column, Memories, the repeater search, and APRS
  weather. Every transmitted value stays native; only the display converts. From F4MQS.
- **Hide stations you've already confirmed on this band.** A new −Conf switch on Band
  Activity drops stations you hold an award-grade confirmation from (LoTW or card) on the
  current band, so you can chase what you still need — while a station that's still new on
  the band always shows. Asked for by F4MQS.
- **Hide callsigns by name or prefix.** A new "Hide calls" control on Band Activity takes a
  list of callsigns or `VP8*`-style prefixes and drops them from the panes — a display
  filter, separate from the block list your auto-CQ honors. From F4MQS.
- **A real blocklist — and your auto-responder honors it.** Alt-double-click a decode or
  roster row and the call goes on a persistent blocked list (it survives restarts; the
  same list is editable under Settings ▸ Modes ▸ Auto-CQ & Caller Selection). While you
  run CQ, a blocked station answering you is passed over for the next caller — with every
  caller blocked, the run keeps calling. Base-call matched, so blocking PD2BS also covers
  PD2BS/P. On screen, blocked calls render dimmed as before; a new "Hide blocked"
  checkbox on the roster and a −Blk switch on Band Activity remove them entirely — except
  the station you are actually working, which no filter ever hides. Asked for from the
  field ("make sure that list is referenced when auto responding to my CQs").
- **Pause the country filter without losing your ticks.** The Countries menu gains a Pause
  switch: turn the whole exclusion off and back on without re-ticking your list.
- **Hide any DXCC entity, not just the common 18.** The Countries menu gains an "Other
  country…" search over the full entity table, alongside the quick-pick list. From F4MQS.

- **The band system now reaches 24 GHz.** The band table, the frequency pickers, the
  override editor and the typed-dial entry all know 33 cm, 13 cm, 9 cm, 6 cm, 3 cm and
  1.25 cm — ADIF's registered names, so a QO-100 contact finally logs `BAND:3cm` instead
  of an empty field LoTW rejects. US transmit privileges follow the regulation exactly:
  Technician-and-above segments for 33 cm, both 13 cm segments (the 2310–2390 gap between
  them stays locked — it isn't amateur spectrum), 6 cm, 3 cm and 1.25 cm; 9 cm carries a
  band label but no US privileges (that allocation was removed), so it reads TX LOCKED for
  US classes and works normally for Open-class operators. Each license class's band
  dropdown shows only its own bands, exactly as on HF. The microwave bands also join the
  per-band grid tracker as the real ARRL VUCC bands they are (5 grids each at 2.3 GHz and
  up). A provably same-band 10 GHz satellite split is now allowed on the Main/Sub Icoms
  (both legs are named and equal — the old refusal said "could not confirm", and now it
  can); a cross-band or off-table split still refuses. Above 24.25 GHz everything still
  fails closed. From the QO-100 field report.
- **The Yaesu FT-890 is in the rig picker by name.** It worked before only if you knew to type
  its Hamlib model number.

### Changed

- **The connector dots tell you whether your contacts are actually getting out.** Settings ▸
  Connections painted every dot from "is a password saved?", which is not the question anyone
  opens that panel to ask. Revoke your ClubLog app-password, rotate a QRZ Logbook key, mistype
  an HRDLog upload code — the secret is still in your keychain, so the dot stayed green while
  nothing reached the service. Each row now reports the last time Nexus really talked to that
  service: **working** with when it last got through, **failing** with the service's own
  reason, **paused** when ClubLog's auth kill-switch has tripped (which until now only ever
  appeared as one line in a log you had probably scrolled past), and **stored — not verified
  yet** in amber for a credential nothing has been sent through. That last one is the point:
  no news is not good news, and it no longer pretends to be. For LoTW, eQSL, QRZ Logbook and
  ClubLog the history comes out of your own log file, so it survives a restart — the panel has
  real history the moment you upgrade. HRDLog.net and Cloudlog leave no such trace, so they
  read "not verified yet" after each restart until your next contact goes out.
- **"Sync LoTW now" said the wrong thing and is now "Download confirmations".** It only ever
  pulled confirmations *down*; operators reasonably read "sync" as two-way and believed their
  contacts had gone to ARRL when they had not. The button now says which direction it goes,
  and points at **Upload to LoTW (N)** in the Logbook for the other one.

- **SSTV now respects your high-duty power cap, not your SSB one.** SSTV has no operating mode
  of its own — it runs as Phone — so its power ceiling came from **Max power ▸ Phone**, the cap
  you set for speech. An SSTV frame keys continuously for up to 290 seconds at close to 100%
  duty, which is exactly what the digital cap is for, and RTTY already used it. SSTV now takes
  the **lower** of your digital and phone caps, so it can only ever come down, never up.
  Worth watching your PA the first time you send a picture.
- **Japan's APRS channel wants a local check.** Nexus derives your APRS channel from your grid,
  and ships 144.660 for Japan while its own band-pack data says 144.640/660. If you're in JA,
  confirm the channel before you beacon — the picker changes it in one click.
- **SSTV has a Settings section.** It was the only mode with a cockpit and no settings, so
  every choice you made — transmit mode, drive — was gone by the next launch, and the one
  thing that did persist (the ISS pass auto-arm) was filed under Rig & CAT, where nothing else
  is about a satellite. Settings ▸ Digital ▸ SSTV now holds all of it: a **default transmit
  mode**, so a station that always sends Scottie 1 stops re-picking it; an **SSTV transmit
  power** that is remembered and applied at Send — leave it blank and Nexus never touches your
  power, which is what it has always done; and a switch for **starting the receiver when the
  SSTV screen opens**, on by default, for operators who keep SSTV up as a monitor on a shared
  rig. Your callsign is still burned into every transmitted picture and still has no off
  switch: an SSTV over carries no other identification.
- **APRS remembers your station.** The beacon symbol, comment, digipeater path and the RF
  channel lived in the screen and reset on every restart, so a European operator's rig was
  retuned to the US channel each launch and their beacon identity retyped each session. All of
  it now persists, under Settings ▸ Digital ▸ APRS ▸ **Over the air**. The channel is
  **derived from your grid square** when you have not picked one — open APRS in Europe and it
  lands on 144.800 with nothing configured, and fixing your grid updates it without a restart.
  There is also a **beacon SSID** control for the first time (-9 mobile, -10 iGate, -7
  handheld): leave it on **From my callsign** and Nexus sends exactly what your callsign
  already spells out, which is what it has always sent. Beaconing gains the two
  alternate-table symbols a fixed station wants, **digipeater** and **iGate**.
- **Settings has a tab for each way you operate — Phone, CW and Digital.** The single Modes
  page held eleven sections, so the mode you came for sat behind every other mode's: a CW
  operator scrolled past the whole FT8 section and six weak-signal modes to reach their keyer.
  It now splits into the three the left-hand rail already shows. SSTV, APRS, RTTY, Tempo and
  the weak-signal modes (Q65, MSK144, JT65, FST4, WSPR) each keep their own named section under
  Digital. The Frequencies tab folds in there too, honestly renamed **Working frequencies
  (FT8/FT4)** — that table only ever held FT8 and FT4 rows, while its name promised every band
  plan you own.
- **The audio settings sit with the COM port again.** Picking the serial port and picking the
  sound card are the same job — on a one-cable interface they are the same cable — but they had
  drifted more than a thousand lines apart, with the satellite and rotator settings in between,
  so setting up a rig meant scrolling past hardware you may not own. Audio now renders directly
  under the CAT settings, the way WSJT-X and fldigi have always had it.
- **Rig Control is now "Rig & CAT", and holds only that.** It had quietly become the place
  everything rig-adjacent landed. Band-edge tones, the per-mode power caps, the setup backup,
  rig sharing and the permission for other programs to key your transmitter move to a new
  **Transmit limits & sharing** section. Nothing changed about what they do.
- **Accessibility settings moved to Appearance**, beside text size and density, instead of
  living under Spots & Alerts.
- **Settings uses the whole window.** It rendered in a 1100px column down the middle, so on any
  ordinary monitor a page that would have fitted was scrolled instead, with half the screen
  sitting empty beside it. Settings now fills the window and lays its sections out in columns as
  the room allows. Controls that gain nothing from being wider — a rig name, a dropdown, a line
  of explanation — are held to a readable width rather than stretched across the glass, and
  nothing changes at all on a small laptop screen, where the column was never the constraint.

### Fixed

- **SSTV on an FM channel now transmits in FM, not USB-D.** Reported from an FTDX10 and an
  IC-9700: pick the 144.500 SSTV channel and the radio goes to FM, then press Send and it jumps
  to USB-D — an SSB signal on an FM repeater. Two separate faults did that. Sending an image
  asked only *which sideband*, so it overrode FM entirely; and Send itself re-entered the Phone
  section, which released the FM channel it had just been put on. An image on an FM channel is
  now sent in the rig's FM data submode (FM-D / PKTFM), so the sound card still reaches the
  modulator and the emission stays FM, and the repeater shift and PL tone stay applied through
  the picture. A radio that does not have FM-D is left in plain FM — never a sideband.
- **The line under the SSTV waterfall stops naming a frequency you are nowhere near.** Sitting on
  a 2 m repeater, it said "Images on this band appear at 145.800 FM" — the ISS downlink, most of
  a megahertz away. It now says that only when the calling channel is near enough to describe
  where you are.
- **The SSTV status line names the control you actually have to find.** It said "Audio input";
  the setting is called "Input Device (RX)".
- **Test CAT now tells you when it couldn't run, instead of pointing at your settings.**
  Pressing Test CAT while the radio engine was down — its sound-card open failed at launch,
  which silently takes CAT down with it — reported "No CAT status yet — set your rig + PTT
  method, Save, then test", sending you to re-check settings that were fine. A test that gets
  no answer now says exactly what is known, including the engine's own error when it left one.
  A Test CAT pressed around a settings Save is no longer silently swallowed by the rebuild —
  it runs right after. From the QDX-on-Linux report.
- **Linux: the sound card you picked actually opens now.** Since 1.0.1 the device menu
  listed cards correctly, but the code that opened your pick checked it against the audio
  library's probe list — and the probing held each card open, so a card's `plughw` entry
  was "busy" against its own `hw` entry, your saved pick never matched, and Nexus silently
  fell back to the default device (the capture loop akhepcat documented; the tune tone on
  the PC headphones instead of the CM108 on mw0cqu's FT-847). Devices are now probed one
  at a time and released between probes, and picking one card for both input and output
  shares a single open instead of fighting itself. And when the menu saved a card's
  `plughw:CARD=…` name but the audio library only ever offers that same card as
  `hw:CARD=…` — a different access path to one physical device, which is what mw0cqu's
  capture showed — Nexus now matches by the card itself and opens it anyway, instead of
  reporting a card that is plainly there as "not available". (#2, #8)
- **A DXpedition Fox's two-in-one message is recognized outside Hound mode.** A Fox packs two
  messages into one transmission — `KR4FQG RR73; W3DIY <YS/WE9G> -06`. In ordinary FT8
  that line never highlighted, even when the RR73 half was ending *your* QSO. It now lights up
  when either half is addressed to you, counts as the sign-off it is, and the row's country and
  worked-before read the Fox's callsign. Display only, deliberately: auto-sequencing from a Fox
  frame outside Hound mode is a separate question. From KR4FQG.
- **A decode addressed to a hashed or portable form of your call highlights as yours.**
  `<YS/WE9G>` when you are YS/WE9G, `W1ABC/P` when you are W1ABC — the auto-sequencer
  already answered these forms; now the decode row shows them as directed to you, the way
  WSJT-X does. From KR4FQG.
- **The logbook edit form can view, correct and add a QSO's POTA park.** The park showed in
  the logbook's Park column but the edit dialog had no control for it, so a missed or wrong
  park number meant editing the ADIF by hand. Edit now carries Park (worked) and Park (mine)
  inputs — and an edit no longer risks dropping a QSO's other OTA/IOTA references.
- **ClubLog uploads no longer hammer, and catch up after you fix the password.** A missing
  ClubLog application password used to retry every two seconds about twenty times with the
  same error and then silently drop the contact; fixing the password retried nothing. Now a
  missing credential pauses ClubLog for the session with one notice, genuine failures back
  off, and entering the password re-queues every QSO that never uploaded. From F4MQS.
- **QSOs logged while HRD is closed are no longer lost.** The push to HRD Logbook was
  fire-and-forget — if HRD wasn't running, the contact vanished. Nexus now queues it and
  sends when HRD is reachable, with a reachable/queued indicator under the HRD setting.
- **A portable-callsign warning for QRZ.** A QRZ logbook is tied to one exact callsign, so
  a /P operator uploading against their base-call book has every contact rejected. Test
  Connection now compares the book's owner to your station callsign and warns plainly, and
  QRZ's terse "Unable to add QSO" errors get a plain-language explanation. From F4MQS.
- **Clicking a spot no longer sends the radio haywire.** On a rig with no data submode — an
  FT-950 was the report — clicking a spot asked for a mode the radio refused, and Nexus kept
  asking: it re-commanded the dial about fifty times a second, indefinitely. The frequency
  readout stopped following the VFO knob, the S-meter froze, and the radio fought you for the
  dial. Nexus now stops re-asking once the rig has refused a mode, and changing band no longer
  re-asks for a mode that was already refused. From the FT-950 report.
- **Changing the audio device no longer risks taking the app down.** Swapping the sound backend
  — an ordinary device change, or switching radios — could collide with the device enumeration
  that runs whenever Settings opens or Detect is pressed, and the collision could kill the
  process outright, with no error and nothing in a log. The two now take turns. This is not the
  startup crash reported against 1.2.0, which is still open.

## [1.2.0] — 2026-08-10

### Added

- **FT8, FT4 and CW work through satellites now.** Hold a transponder and switch to the FT
  console or the CW cockpit: the dial stays on the bird (section entry and FT8↔FT4 tier
  flips no longer re-home it), the rig lands in the right form on the bird's sideband —
  DATA for the digital tiers, CW/CWR for the keyer — and Doppler keeps correcting. For the
  slot modes, corrections now land only in the quiet part of each slot, so a receive period
  is never smeared mid-decode; CW keeps continuous correction, the way dedicated satellite
  programs run it. A QSO logged on a digital tier records the tier's own mode (FT4, not
  SSB) alongside the automatic PROP_MODE=SAT tag, and a held bird now shows a SAT chip in
  the operating cockpits with a release button — the dial never moves with an invisible
  owner.
- **Hide worked-before stations from Band Activity.** A new −B4 switch beside the filter
  chips ANDs with whichever filter is active — CQ-only minus B4, as requested from the
  field. It leaves your own lines and your current QSO partner alone, and goes idle on the
  B4 chip (whose whole job is showing worked stations).

### Fixed

- **Leaving the FT screen and coming back no longer wipes your roster and decodes.**
  Checking the Logbook mid-session and returning froze the app for a couple of seconds,
  cleared the Call Roster and Band Activity, and started them over — the return path was
  re-issuing the tier, and the 1.1.0 stale-cycle protection (which rightly clears
  everything on a *real* FT8↔FT4 switch) fired when nothing had changed. A same-tier
  switch is now a complete no-op: navigate anywhere and back, and your decodes are
  exactly where you left them. Reported with a clean repro by kr4fqg.
- **Band Activity's empty message now says which kind of empty.** With a filter chip lit
  (say, "To me") on a busy band, the pane claimed "No decodes yet — waiting for the next
  slot" while hundreds of rows sat hidden by the filter — which read as a dead decoder.
  It now names the filter and the hidden count.

- **Phone and CW now show TX and live meters whenever the transmitter is actually keyed.**
  The TX badge and the SWR/ALC/power pane watched a flag only the FT8/FT4 transmitter set,
  so voice and CW overs never lit them — the readings were being polled and shown to
  no-one. Both now follow the real transmit arbiter. And Nexus now asks the rig for its own
  PTT once a second while idle, so keying the radio from the radio — mic PTT, a straight
  key — finally shows as TX with live meters too. (#57)
- **WSPR spots resolve their country from the callsign again.** A WSPR line reads
  CALL GRID POWER, but the decode feed parsed it with the FT8 grammar — the trailing power
  figure looked like a signal report, the grid slid into the callsign slot, and TI4JWC's
  Costa Rica became "Armenia" off the EK70 grid. (#55)
- **Old transmitted calls no longer resurface under the live period.** On a busy band the
  decode pane's history cap could evict your own older TX lines, and the next poll re-added
  them stamped into the current period with their original clock time. They now pin to the
  period their transmit time names. (#15)
- **Switching FT8→FT4 on a band with no FT4 calling channel stays put** instead of falling
  back to the top of the plan — which used to drag a 70 cm station to 80 m. The specialty
  modes (MSK144, Q65, FST4) keep their jump to their own calling channel.

## [1.1.0] — 2026-08-10

### Fixed

- **Picking an SSTV frequency now puts the rig in the right mode, whatever screen you came
  from.** The SSTV view deliberately doesn't touch the radio when you open it (it decodes
  wherever you're tuned) — but that meant picking a frequency from its band strip left the
  *previous* screen's mode policy in charge, so arriving from RTTY could land 20 m SSTV in
  DATA-L instead of USB. An SSTV pick now claims the Phone section and tunes in one step:
  you listen (and talk) in plain USB/LSB, and the DATA mode is only commanded while an image
  is actually transmitting — then it hands the mic back. The ISS 145.800 downlink and the 2 m
  FM calling channel now correctly command FM too.
- **Double-clicking a station in the Call Roster now moves your RX/TX to where they were
  heard** — exactly like a Band Activity double-click. The roster remembers each station's
  last decode offset; Hold Tx is respected as always (RX follows, TX stays put when held).

- **Sharing your radio with VarAC, WSJT-X or a logger is now solid — and on by default.**
  The address under Settings ▸ Radio ▸ *Share this radio* is now answered by Nexus itself from
  its live radio state, instantly. Until now it pointed at the underlying `rigctld`: on a busy
  serial link its reply could arrive late enough that the other program asked again and then
  read two answers stuck together — VarAC's "Input string was not in a correct format" — and
  the link dropped whenever Nexus tested or reconfigured CAT. None of that can happen against
  the new address: shared programs stay connected through Test CAT and every settings save, and
  their connections show in the connection log. Long command spellings (`\get_freq` and
  friends, which VarAC sends) are now understood too. Shared programs can still key the radio —
  turn that off in the same block if you want them read-only — and every Nexus transmit
  safeguard applies to them regardless. Upgrading from 1.0.4 or earlier turns sharing on
  automatically; if you installed 1.0.5, flip it on once in Settings ▸ Radio.

- **Two identical radios no longer get pointed at the same sound card.** With two rigs whose
  USB audio shows up under the same name (two Yaesus both presenting "USB Audio CODEC"),
  Detect used to suggest the first sound card for both — so radio 2 transmitted into radio 1's
  audio, silently. Sound cards are now handed out one-per-radio: the worst case is the two
  swapped (visible, and one question to fix), never shared. Auto-test also skips ports that
  belong to your other configured radios instead of spending its time probing ports it can
  never open.

- **The radio-control helpers no longer listen on your whole network.** The `rigctld` and
  `rotctld` servers Nexus starts accepted connections from any machine on your LAN, while the
  share UI promised "this computer only". Both now bind localhost, matching the promise —
  sharing between computers was never a supported path, and rig control is not something to
  leave open on hotel Wi-Fi.

- **Clicking a spot no longer bounces the rig off a default frequency on the way.** Working an
  FT4 spot while on FT8 (or the other way round) used to switch the mode first — briefly
  commanding the mode's standard calling frequency — and then jump to the spot. On a slow CAT
  link the two commands could land out of order, intermittently leaving the radio on the wrong
  frequency or mode after the click. The mode switch and the QSY are now one atomic command:
  the rig hears only the spot's exact frequency. The same click from the torn-off Needed board
  also now switches FT8/FT4 to match the spot — it previously left the decoder on the old mode.

- **The hourly QRZ logbook sync no longer wipes out a QSO in progress.** With auto-sync on,
  the timer's bookkeeping ran through the same heavyweight path as a full Settings save, which
  resets the operating state and clears anything queued to transmit — once an hour, mid-QSO,
  with nothing on screen saying why. The sync now records its progress without touching the
  operating state.

- **Band changes no longer let the radio pick the wrong mode.** The big one from the bench:
  selecting 12 m or 30 m in the CW section landed the rig in DATA-U; picking 20 m back in the
  FT8 section landed it in CW-U. The radio itself was doing it — modern rigs keep a per-band
  last-used-mode memory, and when Nexus commands the mode before the dial (required since
  1.0.1 to stop the FTdx10's 650 Hz CW-pitch walk), a band-crossing dial write let the rig's
  own memory override the mode Nexus had just set — and Nexus, believing its command stood,
  never checked. Now every band-crossing retune reads the rig's real mode back and re-asserts
  once if the rig overrode it, then re-seats the dial. Your click wins, on every band, in
  every section — and the pitch-walk fix stays intact. (This was in 1.0.1 through 1.0.5 too:
  it is the "intermittent wrong mode" some operators reported, deterministic per band.)

- **A Settings save can no longer flip you out of your operating section's mode.** The
  operating mode is live section state now, like the dial: a Settings form saved after you
  switched sections carried the old mode and silently reverted the rig. Every stale-save
  door is closed at once.

**Awards — a tester put their 31,000-QSO log next to their official ARRL account, and every
difference below is chased to the award's actual rules:**

- **5-Band DXCC now computes the award ARRL actually grants.** It used to count entities
  confirmed on *all five* classic bands at once; ARRL wants 100 on *each* band, and they don't
  have to be the same 100. The tile now shows your weakest band — the number that decides the
  award — and says so. 5-Band WAS follows the same per-band rule.

- **Satellite contacts no longer inflate your band DXCC.** A QSO tagged as satellite counted
  toward 2 m DXCC and Mixed, which ARRL restricts to the satellite awards — one bird QSO was
  enough to make Nexus disagree with your LoTW account by one on 2 m. Satellite entities now
  stand in their own Satellite DXCC count, shown on the Sat VUCC card, matching LoTW's
  Satellite column.

- **VUCC is now the real award, not a grand total.** The old tile divided your all-band grid
  count (mostly HF FT8 grid exchange) by 100 and called it VUCC. The award is 50 MHz and up,
  judged per band with per-band thresholds — 100 on 6 m or 2 m, 50 on 222/432 MHz, 25 above.
  The tile now shows your best real standing ("VUCC ✓ 6m") and keeps the all-band count
  visible as the grid tracker it is. Terrestrial 70 cm and 1.25 m grids — real VUCC bands that
  previously counted for nothing — now count.

- **IOTA's checkmark now follows IOTA's own rules.** The programme credits QSL cards and
  Club Log matching, never LoTW — so a LoTW-only confirmation still tracks your progress but
  no longer lights the award. The card shows your card-confirmed count beside it.

- Clearer sentences where the numbers were right but the words weren't: Honor Roll now says
  "1 more confirmed needed — entry at 331" instead of the ambiguous "1 confirmed to Honor
  Roll"; the Confirmed tile says it counts LoTW-or-card only; the Sat VUCC card now states on
  screen that it counts PROP_MODE=SAT-tagged contacts — which Nexus now writes automatically
  when you log on a bird's downlink (see below).

- What is *not* changed, on purpose: Nexus computes from your log, so it will honestly differ
  from an ARRL account that includes decades of paper-card credits applied at ARRL or QSOs
  from merged callsigns that aren't in the imported file. Import your full history and your
  LoTW report to close most of that gap.

**Installing:**

- **The installer no longer looks frozen, and the docs no longer send you to install
  WebView2 by hand.** Everything Nexus needs ships inside the installer — WebView2, Hamlib,
  the DSP stack — and always has. But the 209 MB WebView2 step ran silently (minutes of
  apparent hang), and an old troubleshooting page still said to go download the runtime
  yourself, which is exactly what a new operator did (IK6HQL's report). The WebView2 step now
  shows its progress, both troubleshooting pages say plainly that nothing needs installing by
  hand, and the release pipeline now proves the WebView2 payload is inside every installer it
  publishes.

- **The installer speaks Italian** (and German, French, Spanish, Portuguese, Dutch and
  Japanese) — it asks your language up front instead of assuming English.

- Download docs corrected: GitHub Releases listed alongside SourceForge (a generator slip
  had listed SourceForge twice and GitHub never), the documented size matches the real
  ~250 MB, and the README's file table no longer hardcodes an old version number.

- **Answering a station right after a mode switch could transmit on their own cycle.** Switching
  FT8 to FT4 (or back) changes the length of the transmit periods, so everything Nexus knew about
  who transmits in which half is discarded — correctly. But the Stations list survived the
  switch, still offering every station from the old mode, and answering one silently kept
  whatever cycle you were already on. If that was the DX's own cycle, you transmitted every time
  they did: they could never hear you, and nothing anywhere said so. From the same POTA field
  report as Field mode.

  Three things changed. The Stations list now clears on a mode switch, exactly as the decode
  panes always have — it refills with the first decodes in the new mode. Clicking a leftover
  decode line that can no longer be placed on the right cycle is refused with a message telling
  you to wait for the station's next transmission, instead of calling blind — the wait is one
  decode long. And typing a callsign in by hand still works exactly as before, on whatever cycle
  you have set, which is what WSJT-X does with a typed call too.

- **You can now see which transmit cycle the app picked for you.** Double-clicking a station has
  always set your cycle opposite to theirs — that part worked — but the big Tx 1st / Tx 2nd
  buttons only light when you lock a cycle by hand, so the automatic flip showed only as tiny
  text and looked like nothing happened. The side the app is currently on now carries a clear
  underline, distinct from the lit look of a manual lock, so a working flip finally looks like
  one. From the same field report as Field mode.

- **Contacts broadcast to N1MM+ or N3FJP now say which radio made them.** Every packet claimed
  radio 1, so a two-radio station had its whole session attributed to one rig — and silently,
  because a logger receiving the feed has no way to tell a wrong radio number from a right one.
  Nothing looks broken until someone reads the log and the bands make no sense. Both the ordinary
  log broadcast and the Field Day feed now carry the radio that actually made the contact. A
  single-radio station is unchanged: still radio 1.

- **Changing band no longer leaves the radio on the wrong sideband.** Picking 20m while you were
  on 40m carried LSB across with you: the menu offered 14.230 USB and the rig was commanded to
  14.230 LSB. Typing a frequency by hand did the same thing. Transmitting on the wrong sideband is
  as bad as transmitting on the wrong frequency — nobody hears you, and nothing tells you why.

  Crossing 10 MHz now follows the usual convention, LSB below and USB above. Moving around
  *within* one side still keeps whatever sideband you are on, so if you have deliberately put the
  rig on USB down on 40m for digital work, an ordinary retune leaves it alone. FM, AM and the data
  submodes are untouched. Reported by patpell.

- **KG4 callsigns are no longer all credited to Guantanamo Bay.** Only a KG4 call with exactly two
  characters after it — KG4AB — is Guantanamo; KG4ABC and KG4A are ordinary United States calls
  sharing the same block. Every KG4 was being filed as Guantanamo, which is a rare entity, so it
  showed up as a burst of activity from a place that was not on the air and could put a wrong
  entity in your log and on the Needed board. The country file names some of these exceptions
  individually and those are still honoured — a few KG4 holders really are in Alaska or Hawaii —
  but it can only list the calls it already knows, so the rule is now applied by shape as well.
  Reported by graafpeter-web.

- **A callsign with a stroke in it resolves in the callbook again.** Looking up `W1AW/1` found
  nothing, while clicking through to QRZ loaded the record straight away — which made it look like
  Nexus could not find a call QRZ plainly has. The two are genuinely different: QRZ's website
  redirects a stroked call to its base, and the data service Nexus queries does not. Nexus now asks
  for the call exactly as you typed it and, only if no callbook has it, asks again for the base
  call. That order matters — a portable or club call can hold a record of its own, and looking up
  the base first would hand you a different operator's details without saying so. Reported by
  kr4fqg.

- **Your memory channels, watchlist and chase lists were being kept somewhere that does not
  survive a reinstall.** They lived in the browser storage inside the app window, along with your
  profile list, your armed satellite and DXpedition alarms, and your UI scale — which is an
  accessibility setting, so losing it is not a cosmetic matter. None of it sat beside your
  settings file, none of it was covered by a backup of that file, and none of it was per-profile.
  Reinstalling rather than upgrading in place could clear the lot, with no warning and no way
  back.

  All of it now lives in `ui-state.json`, next to your settings, and is carried across
  automatically the first time you run this version. Nothing is deleted from the old location, so
  going back a version loses nothing either. Genuinely cosmetic things — a collapsed panel, the
  tab you were last on — stay where they were, which is what that storage is for.

- **Some rigs were keyed the moment Nexus connected, whatever you set PTT to.** Reported on a
  TS-2000 with a Digirig, and it is not specific to either: about fifty radios tell Hamlib they use
  hardware flow control on the serial port — the Kenwood TS-2000, TS-590 and TS-990S, the Yaesu
  FTDX10, FT-991 and FT-891 among them. On those, the RTS line belongs to flow control and sits
  raised for the whole session, and Hamlib refuses to let anything hold it down. On the very common
  interfaces that key from RTS — Digirig and most homebrew cables — a raised RTS *is* transmit. So
  the radio went into transmit as the port opened and stayed there, which is also why changing the
  PTT setting made no difference: flow control is not keying, so no PTT choice ever touched it.

  Nexus now turns that flow control off — but only when RTS on that port is genuinely wired to
  keying: a recognised keying interface (the Digirig class) or a serial PTT/CW line you have
  configured there. A rig on its own plain USB CAT cable keeps its hardware handshake untouched.
  Hardware flow control is close to meaningless for the short messages CAT exchanges — WSJT-X
  drives these same radios without it — and a radio that transmits unattended is the worse
  outcome by a distance. Reported by vk6mo.

- **The Linux .deb installed cleanly and then would not start.** The package named Hamlib, WebKit
  and GTK as its dependencies but never mentioned the Fortran runtime or single-precision FFTW
  that the modem is linked against — so `apt` reported success, the menu entry appeared, and the
  binary then died on `libgfortran.so.5: cannot open shared object file` the moment you clicked
  it. Nothing in the install said anything was missing, because as far as `dpkg` was concerned
  nothing was. It only ever worked by luck: on a machine that happened to have both libraries
  already, for some other reason. The package now declares `libgfortran5` and `libfftw3-single3`,
  so `apt` installs them with Nexus — and on a system that genuinely cannot supply them, the
  install refuses up front instead of handing you an application that cannot open.
  Reported and fixed by Justin Johnson (G0KSC).

- **macOS: RX audio could go permanently silent with no error shown.** The bundle shipped with no
  `NSMicrophoneUsageDescription` in `Info.plist`, so macOS could never show the audio-input
  permission prompt at all. Once the OS records a denial for the app's bundle ID, CoreAudio does
  not surface that as an error — the input stream opens normally and the audio pipeline runs, it
  just delivers silence, which looked identical to a device-selection or gain problem. Added
  `src-tauri/Info.plist` with the usage description so macOS can actually ask; anyone hitting this
  on an existing install also needs `tccutil reset Microphone com.kd9taw.tempo` once to clear the
  stuck denial before the next launch will prompt again. Reported and fixed by g0fqb. Applies to a
  build from source — macOS is not a shipped platform.

### Added

- **Satellite contacts tag themselves now.** Log a QSO while a transponder is held and your
  dial is on that bird's downlink, and the record gets `PROP_MODE=SAT` plus the satellite's
  LoTW designator (`SO-50`, not the catalog name) — written as a pair, which is what TQSL
  demands. Your pass QSOs now count toward Satellite VUCC and the new Satellite DXCC figure
  in the app, and upload to LoTW as creditable satellite contacts, with nothing to edit by
  hand. The passband check means an HF contact made while a bird is still held from an
  earlier pass is never mistagged. One honest exception: ISS contacts stay untagged (there
  is no LoTW designator Nexus can safely derive from the catalog name) — add the two fields
  yourself for those.

- **The wizard no longer asks what you want to do — you get everything.** The goals step
  (pick DX chasing / POTA / contesting…) is gone: every mode and every section starts ON,
  because Nexus is one program instead of six and there is nothing to unlock. The final step
  is now just your license class, the optional starter channels, and the walkthrough offer.
  The goal profiles still exist in Settings ▸ Appearance for anyone who wants a leaner app.

- **The top bar got its space back, and Light/Dark moved home to Settings.** On an ultrawide
  the chips used to pile against the right edge with a dead zone after your callsign; in a
  snapped window they wrapped to a stray row. The chip cluster (operator, Help, Field) now
  sits with the mode pills on the left, ahead of the TX clock, so the slack lands at the end
  of the bar instead of inside it — and the buttons themselves slimmed down. The Light/Dark
  theme switch is a set-once preference, not an operating control, so it now lives in
  Settings ▸ Appearance instead of spending top-bar space on every screen.

- **The setup wizard now finds your radio instead of waiting to be told about it.** Open the
  rig step and the scan is already running — USB radios and FlexRadios on the network appear
  without pressing anything. A radio behind a generic USB cable (no model in its USB name) is
  identified by probing the common rigs at their real baud rates, with a plain elapsed counter
  while it works. When the probe finds the port but has to guess the model — an FT-991A
  answers the same probe as an FTdx10 — the wizard says so and asks *which radio is this?*
  instead of quietly saving the guess; picking a fixed-rate rig sets its one true baud
  automatically. Radios configured here now key via CAT instead of the old silent VOX default
  that made the wizard's own Test CAT report failure. And the step ends with the same live
  Setup-health strip Settings has — Rig / RX audio / TX, with Prove TX — so you leave the
  wizard on evidence, not hope.

- **A second radio is one button in the same step.** "I have a second radio" adds it, probes
  the remaining ports (your first radio's port is skipped — it's busy being your first radio),
  and saves the port, speed and its own sound card to the new radio's profile. If both rigs
  use identical USB sound cards, they are shared out one each and the wizard tells you the
  one thing to check — and gives you a single *swap them* button if the wrong rig's meters
  move. It also finally says out loud: run both radios at once by opening Nexus twice.

- **Field mode — one tap for operating outdoors.** A POTA activator reported the screen was
  very difficult to read outdoors even shaded, in either theme, and he is right about "either":
  daylight on the panel washes out low-contrast text first, and about two-thirds of the text in
  Nexus is deliberately de-emphasised grey — that, not the type size, is what disappears in the
  sun. The new **Field** chip in the top bar turns the de-emphasised text near-solid, pushes
  the surfaces to full black-and-white (in whichever theme you are in), and steps the whole
  interface up a size or two to match what your window can hold. Tap it again and you are back
  exactly where you were — it never touches your theme or your saved scale. Pop-out windows
  follow it. The tooltip will tell you the Light theme reads best in daylight, but the choice
  stays yours.

- **You can skip the burned-in callsign for a picture that already shows it.** A pre-made QSO
  card carries your call as part of the artwork, and the burn-in was covering it to repeat it.
  Tick *My picture already shows my callsign* under the transmit preview and the plate is left
  off — for that picture only. The tick clears every time you load a new image, on purpose: it is
  a statement about one picture, and identification is your responsibility when you make it —
  Nexus cannot read your artwork. With the box unticked nothing changes, and a station with no
  callsign set still cannot transmit at all. Asked for by akhepcat.

- **Back up your whole setup to a file, and restore it on another computer.** Settings ▸ Radio
  now has Back up and Restore. One file holds your radios, operating preferences, memory
  channels, watchlist and chase sets — for a new laptop before a contest, a rebuild after a disk
  failure, or a second machine that should match the first. Until now there was no way to keep a
  copy of any of it: it lives in a configuration folder most operators never open.

  **It holds no passwords or API keys.** Those stay in your operating system's keychain, so a
  restore asks for them again and the file is safe to carry on a USB stick or email to yourself.
  Your contact log is separate and is not included — export that from the Logbook. Restoring
  replaces your current setup and says so before it does anything.

- **The operator at the key is shown in the top bar, and swaps in one click.** Set an operator and
  it appears beside Help, in the one group no cockpit hides — because a wrong operator is silent:
  nothing misbehaves and you find out at submission, when the log is already wrong. Click it to
  hand over to anyone already in the log, or to go back to single-operator when the activation
  ends. Nothing shows at all if you have not set one, which is the normal single-op case.

- **Export one ADIF per operator.** With more than one operator in your log, the Logbook grows an
  *Export per operator* button: one file per operator, named with their callsign, plus the
  combined log — which is still the one carrying any contacts logged with no operator set. POTA
  and Field Day both want each operator to submit their own, and these get uploaded from a phone
  in a car park, so the filenames say who they belong to at a glance. A single-op station never
  sees the button, because the file it produced would be a copy of the one beside it.

- **Two of you on one radio? Your contacts can now say who made them.** Set *Operator at the key*
  in Settings ▸ Station and every contact you log carries it, so a shared POTA activation or a
  Field Day shift can be split by operator afterwards instead of hand-edited — both programs want
  each operator to submit their own log. It is the ADIF `OPERATOR` field, which is a different
  question from your station callsign: one is who was operating, the other is whose station it
  was, and that distinction is exactly what the two fields exist for.

  Leave it blank if it is just you and nothing is stamped — an operator equal to the station call
  in every record says nothing. This setting existed before but only inside Field Day, where a
  POTA pair would never have found it, and it only ever reached an N3FJP feed rather than your own
  log. Asked for by a POTA operator running two ops on one laptop.

- **The band map tunes now — click it, or scroll on it.** It has always been a frequency scale
  with your dial marked on it, but it was the one place showing frequencies that you could not
  act on: the readout digits and the waterfall both tune by wheel, so the map staying inert read
  as something broken rather than something missing. Click anywhere on the track to go there, or
  scroll to tune the way every other dial in Nexus does — same step, same sensitivity setting.
  Clicking a spot still works that station rather than tuning to it, because a spot's label is
  nudged aside from its true frequency to keep a crowded band readable, so the label is not where
  the signal is. The map stays read-only while CAT is down or you are transmitting. Asked for by
  kr4fqg.

## [1.0.5] — 2026-08-08

### Added

- **CW can record a contact without switching cockpits.** Phone has had a record button since the
  audio bridge landed and CW never got one, so recording a CW QSO meant leaving the cockpit to do
  it. Same button in the header, same stop in the top bar.

- **You can delete a received SSTV image from inside the app.** There was no delete anywhere — once
  a picture decoded it was permanent as far as Nexus was concerned, and the gallery only ever grew.
  Hover a thumbnail and a ✕ appears; it asks first, naming the picture rather than just "are you
  sure", because the tiles are small and several look alike. It is not recoverable afterwards: a
  received picture is the only copy of something somebody sent you.

- **Settings now shows you where recordings actually go, with a button to open the folder.** This
  is the other half of the same report, and probably the bigger half: the recordings folder lives
  under the config directory *for that radio profile*, so a second radio keeps its recordings
  somewhere else entirely, and the folder is not created at all until the first recording lands.
  Between those two, an operator looking in the obvious place finds nothing and reasonably concludes
  the feature is broken. The decode log has shown its path this way for a while; recordings do now
  too, and the button creates the folder if it does not exist yet rather than doing nothing.

### Changed

- **When an audio device will not open, Nexus now says what it actually found.** The old message
  claimed the device was "missing, or in use by another application" without having established
  either, which sent at least one operator chasing a busy device for two releases. It now names
  the device you asked for, counts and lists the devices the audio backend could see at that
  moment, and explains the case that actually bites: a device can be on the menu and still be
  missing from that list, because the menu and the opening code ask the system in different ways.
  Selecting one sound card for both input and output is the usual way to hit it.

- **Your recordings and your received SSTV pictures now live where you would look for them.**
  Recordings go to **Documents ▸ Nexus ▸ Recordings** and received SSTV images to
  **Pictures ▸ Nexus SSTV**. Both used to sit in Nexus's own configuration folder, which is hidden,
  is not the same place for a second radio, and is not somewhere anyone thinks to look — several
  people concluded recording was simply broken, and they were reasonable to. Pictures you were sent
  are worth being able to find, open and share without going through the app.

  **Your existing SSTV gallery comes with you.** The first time you start this version, the images
  and their index are moved into the new folder, so the gallery looks exactly as it did — nothing
  is left stranded and nothing needs re-importing. Recordings you already have are left where they
  are rather than moved out from under you; only new ones go to Documents. If Windows cannot tell
  Nexus where your Documents or Pictures folders are, it carries on using the old location rather
  than guessing at a path.

  Voice-keyer messages are unchanged. Those are app state rather than something you browse — they
  are referenced from your settings by name, and the keyer already has its own recording controls.

- **The record button is findable again.** It was reduced to a bare dot in a box last week, in a
  header that also carries the band picker, tuning strip, Tune and Stop TX — small enough that it
  was reported as missing before it was reported as hard to see. It says REC next to the dot now,
  and the dot is red at rest rather than only while recording, which is what makes a record button
  look like one. The recording state was also using a red that had never been defined, so it fell
  back to a shade picked for the dark theme and washed out on the light one; it uses the app's
  transmit red now and reads properly in both.

- **The Openings and Band Advisor panes fit more on screen.** Both were set a size larger than they
  needed and were costing more scrolling than the information warranted — about one extra row now
  fits per six. The Openings log was also using hardcoded sizes rather than the app's text scale, so
  it ignored the rest of the sizing system; it follows it now.

### Fixed

- **Contacts never reached N1MM+, HRD or Log4OM unless you were running Field Day.** Nexus speaks
  the WSJT-X UDP protocol on 2237, and loggers pick up your decodes and your status from it — which
  is exactly why this was so hard to spot. The connection looked alive: N1MM's WSJT window filled up
  with decodes. But the one message a logger actually writes a contact from, `QsoLogged`, was only
  ever sent for Field Day contacts. Every ordinary QSO went into your own log and nowhere else, with
  no error and nothing to suggest anything was missing, and the FAQ told you the path was supported.
  Every logged contact now goes out on it — FT8 and FT4, phone, CW, RTTY, SSTV, and rows you type
  into the Logbook by hand.

  Worth knowing if you were chasing this: the **N1MM contact broadcast** in Settings is a different
  thing and was never going to help. N1MM does not read those packets back in — they exist for club
  dashboards and live maps. The 2237 path is the one N1MM logs from.

- **FT8 and FT4 were putting a six-character grid square on the air.** A standard FT8 or FT4
  message has room for four characters of locator and no more, so if you had set a six-character
  grid in Settings, your calls to another station carried something the message format cannot hold.
  The station you were working could not decode it as a grid at all, which means their software had
  nothing to auto-reply to and the contact stalled on their side for a reason that looked like
  nothing at all on yours. Curiously your CQ was fine — that path already trimmed it — so this only
  bit once you actually answered somebody.

  Nexus now sends four characters, the same as WSJT-X, which trims to four in exactly one place for
  exactly this reason. Your settings and your log are untouched: a six-character locator is correct
  in both, and it is only what leaves the antenna that is cut. Reported by kr4fqg.

- **Clicking the waterfall moved your transmit frequency as well as your receive frequency.** The
  hint under the waterfall says left-click sets RX, Shift or right sets TX, Ctrl sets both, and that
  is what WSJT-X does — but a plain left-click in Nexus was moving both markers, so a click meant to
  listen to someone also moved you on top of them. The only way to stop it was to switch Hold Tx Freq
  on, which is a workaround for a bug rather than what that switch is for. A plain click now moves
  the green RX marker and leaves your TX frequency exactly where you put it, whatever Hold Tx Freq is
  set to. Double-clicking a decode to work a station is unchanged and still brings TX with it unless
  you are holding — that is what Hold Tx Freq is actually for, and it is what WSJT-X does too.
  Reported by akhepcat.

- **A station Nexus had given up calling stayed armed to transmit, with no time limit.** When you
  call a station in FT8 or FT4 and it never answers, Nexus stops calling after eight overs. It kept
  the QSO open while it waited, which is what you want — but the TX watchdog, the six-minute limit
  that exists to stop an unattended radio, was only ever checked at the moment an over was being
  built. A held-back over is not built, so the clock was never looked at. The QSO sat there armed
  with Enable TX still lit, and if that station was decoded again later — minutes or hours — Nexus
  answered it without you touching anything.

  On the shipping defaults the give-up always came first: eight FT8 overs is four minutes against a
  six-minute watchdog, so on a called station the watchdog could not fire at all. The watchdog now
  runs while a station is being held back, so the six minutes you set is the six minutes you get,
  and when it expires TX disarms as it does everywhere else. Nothing about the message sequence,
  slot timing or when Nexus decides to stop calling has changed — only that being stopped is now
  bounded by the clock. If you are simply monitoring with TX armed and waiting for a decode, you are
  unaffected: the watchdog still does not start until there is something it is holding back.

- **The Pwr slider lagged several seconds behind what was actually going out, and its useful range
  was crushed into the bottom of the travel.** Moving it only affected audio Nexus had not generated
  yet, and it generates well ahead — an FT8 over is built and queued in one go, all thirteen seconds
  of it — so the level was already baked into everything waiting to go out. Hold Tune, move the
  slider, and the rig's ALC sat where it was for a good few seconds before catching up, which makes
  it very easy to overshoot into compression while chasing a control that has not responded yet. The
  level is now applied to each sample as it leaves for the sound card, so what you set is what goes
  out on the next fraction of a second, including audio already queued. The waveform is scaled rather
  than dropped and rebuilt, so there is no gap or click when you move it mid-transmission. Affects
  every mode that transmits through the sound card.

  Holding Tune made it worse than it had to be: the tune carrier was generated in fixed 40ms chunks
  regardless of how often the driving loop actually ticked, so the queued backlog grew without bound
  for as long as Tune was held and a long hold took a moment to stop. Separately, the Settings
  panel's own Tx Power slider only applied its value on release rather than while dragging, so it
  gave no feedback until you let go, unlike the matching cockpit Pwr slider. Both track live now.

  And now that they do: the drive range that actually matters on real hardware — 0 up to just past
  where ALC engages — turned out to live in only the bottom 15-20% of the old linear slider, so both
  sliders now use a curve that gives that range most of the travel. **What a saved drive level means
  is unchanged — only how far along the slider you move to reach it**, so check your usual drive is
  where you want it before working anyone. Reported by g0fqb, who also found the cause.

- **Two ways the "new band" and "new mode" badges could tell you something was new when it was
  not.** Both came from comparing what the log happens to say against what the radio happens to
  say, as plain text.

  A contact logged on **USB** did not match one logged on **LSB**, so working a country on one
  sideband told you it was a mode you had never worked there — they are the same mode, and ADIF
  says so. That is now folded, along with a couple of spellings of the same digital mode. Nothing
  else is: FM and AM stay separate from SSB, and FT4 stays separate from FT8, because those are
  genuinely different modes even where an award groups them together. Where the log is honestly
  ambiguous — a bare `MFSK` row that could be several things, or the generic `PH` some loggers
  write — it is left alone rather than guessed at.

  Separately, an imported contact whose band field did not name a band Nexus recognises matched
  nothing at all, so the band you were sitting on read as new against it every single time, and no
  amount of operating would ever clear it. The frequency is now used to work out the band when the
  band field itself is no help. When there is neither — old imports often carry no usable band and
  a frequency of zero — Nexus now says nothing rather than claiming the band is new, because it
  genuinely cannot tell.

  Entities are untouched and were already right: European Russia, Asiatic Russia, Kaliningrad and
  Franz Josef Land are four separate DXCC entities and are each tracked on their own.

- **The Needed board said "Digital" for some stations and "FT8" for others, and they were all
  FT8.** Rows reach that board two ways. One carries the real mode and says FT8; the other comes
  from a DX cluster spot, where the only thing worth believing is the frequency on the dial — and
  that path was only ever asking "is this digital, voice or CW?" and showing the answer. But the
  band plan knows more than that on those frequencies: 14.074 is FT8 and 14.080 is FT4, and it has
  always known, the detail was just being thrown away. Spots on a known FT8 or FT4 watering hole
  now say so. Anywhere else there genuinely is nothing more specific to be had from a bare
  frequency, so those still read Digital rather than Nexus inventing a mode. Clicking a row takes
  you exactly where it did before. Reported from the 1.0.3 test build.

- **The Call Roster never showed which station you were actually working.** In a busy roster there
  was nothing to say which of those calls belonged to the contact in progress — and not because the
  highlight was too subtle, but because the roster was never told. It was being handed the Tempo
  chat peer, which is empty for the whole of an FT8 or FT4 session, so nothing ever matched. The
  station the sequencer is working now stands out in the transmit colour, stays legible instead of
  fading with age like the other rows, and reads as "working now" to a screen reader. It is
  separate from the row you last clicked, so you can look at one station while working another.
  Asked for by m7jyfradio and akhepcat.

- **Deleting an image by hand no longer leaves a broken thumbnail — and pictures you copy in show
  up.** The gallery kept its own list of images and never checked it against the folder, so removing
  a `.bmp` yourself left an entry pointing at nothing, which is exactly what you had to do given
  there was no delete. It now reconciles with the folder when it loads: entries whose file is gone
  drop out quietly, and images sitting in the folder that Nexus has not seen before are picked up,
  dated and named from their own filename. Managing the folder yourself works now instead of
  breaking things — which matters more since the gallery moved to Pictures.

- **A QSO recording that could not be saved said nothing at all.** Both steps — creating the folder
  and writing the file — threw their result away, so a full disk, a read-only folder or a
  permissions problem produced no file, no message and nothing in any log. The only evidence was an
  empty folder, which is also exactly what a perfectly healthy Nexus looks like before your first
  recording lands. If a recording cannot be written you now get told, and the message names the
  full path it was trying to write, so you can see straight away whether it is a permissions
  problem or you were looking in the wrong place. The contact itself is unaffected — it is logged
  either way; only the audio failed.

- **The Tempo dial would not scroll.** Hover a digit of the big frequency readout and roll the
  wheel, and that digit steps — the 1 kHz digit by 1 kHz, the 1 MHz digit by 1 MHz, carrying the
  way a real VFO carries. Every cockpit has worked that way for a while except Tempo, which used
  the same readout with the tuning switched off. There was nothing to see: a readout with digit
  tuning and one without look identical, so the only symptom was that scrolling did nothing.

- **The wheel tuning sensitivity slider did nothing on four of the six dials.** Settings ▸ Radio
  says it applies to the frequency readout, and it only reached Phone and CW. On Operate, RTTY and
  SSTV the digits tuned at the stock rate no matter where the slider sat, so anyone who moved it
  because a free-spinning mouse was overshooting got no change and no reason why. It now reaches
  every readout, Tempo's included.

- **macOS: CAT could fail to connect even with Hamlib installed via Homebrew.** A Finder/Dock-
  launched app is started by launchd with a fixed `PATH` of `/usr/bin:/bin:/usr/sbin:/sbin` —
  never the interactive shell's `PATH`, so a Homebrew `rigctld` (`/opt/homebrew/bin` on Apple
  Silicon, `/usr/local/bin` on Intel) or a MacPorts one (`/opt/local/bin`) was invisible to Nexus
  even though it worked fine from Terminal. Nexus now also checks those common install
  directories before giving up, the same way the Windows build already prefers a binary bundled
  next to the app. A `rigctld` already on `PATH` still always wins.

- **The Linux download now says which Linux it needs.** Both PC Linux files require Ubuntu 24.04 or
  newer, and nothing said so — not the download page, not the README, not the package itself. On
  anything older the `.deb` installs without a word of complaint and then the app does not start,
  which is how a report from a Mint 21.3 operator reached us. The AppImage is no help there either,
  despite what portability usually means: an AppImage carries the application's own libraries but
  not the system C library, so it needs exactly the same minimum. The requirement is now stated
  everywhere the files are listed, with the one command that checks it (`ldd --version`).

  Behind that, the build that produces those files was pinned. It had been following whatever image
  GitHub happened to call "latest", so the oldest distro Nexus ran on was never a decision anyone
  made — and the next time that label moved it would have risen again and cut off working
  installations, with a completely green build and nothing to point at. The release now refuses to
  publish a Linux binary that needs more than the stated minimum.

## [1.0.2] — 2026-08-06

### Added

- **SSTV transmit was sending no station identification of any kind. It now burns your callsign
  into every picture.** This is the plain fact and it is worth stating plainly: an SSTV over is one
  continuous key-down of up to about five minutes carrying nothing but picture, and until now
  nothing in that transmission said who was sending it. There was no callsign drawn into the image,
  no CW ident after it, and the FSK ID that some stations append is something Nexus has only ever
  *read*, never transmitted. Your call now appears in the top-left corner of every transmitted
  image, white on a solid black plate, with no switch to turn it off — and **Send is refused
  outright if you have not set a callsign** in Settings ▸ Station. §97.119(b)(4) allows the call to
  ride in the image when the picture is the communication, and since the longest over Nexus can key
  is PD-290 at about 4:50, back-to-back images stay inside the ten-minute rule with room to spare.
  The end-of-communication ident is still yours to send if the QSO finishes on voice or you stop an
  image part-way. The plate is designed to survive the mode rather than merely be present in the
  file — sized as a fraction of picture width so it scales from Scottie's 320 pixels to PD-290's
  800, white-on-black because that is the full tone range an SSTV mode carries, and drawn from a
  fixed bitmap font so the letters are identical on every machine. It is checked by decoding it
  back: the test suite encodes a picture carrying the plate, runs it through the real decoder and
  reads the callsign out of the resulting pixels, for all fifteen modes, clean and at 20 dB and
  10 dB signal-to-noise. What that cannot prove is how another program's decoder renders it — for
  that, ask a station running MMSSTV or QSSTV what they see.

- **Send any picture to SSTV: it is resized, rotated upright and cropped for you.** Before this you
  had to produce a file at the mode's exact pixel size yourself — 320×256 for Scottie and Martin,
  320×240 for Robot, and four other sizes across the PD modes — or the transmit was refused. Drop a
  4032×3024 phone photo now and it just works. **Rotation is read from the file**, so a picture
  taken in portrait no longer transmits on its side, which is the trap that would otherwise have
  bitten every iPhone user and nobody testing on a desktop. The picture is cropped from the middle
  to the mode's shape and scaled down in stages rather than in one jump, so fine detail — foliage,
  brickwork, fabric — softens instead of breaking into the crawling speckle that a single-step
  resize produces and that an SSTV transmission then faithfully sends for four minutes. **Drag the
  preview to choose which part of the picture is sent**; arrow keys nudge it, shift moves ten
  pixels, Home or a double-click re-centres. Only the axis with something to give will move, and
  when the picture already matches the mode it says so rather than offering a control that does
  nothing. Change the mode and the crop re-derives at the new shape, keeping your framing. JPEG,
  PNG, WebP, BMP and GIF are accepted (a GIF sends its first frame); a picture smaller than the mode
  is enlarged with a warning that stays on screen rather than being refused. **iPhone HEIC photos
  are refused by name**, with both of the fixes on the phone spelled out, because Nexus has no HEVC
  decoder and "could not load that image" would have sent you looking in the wrong place. The
  composer now also tells you the original size, the size it was resized to, and — before you key
  anything — exactly how long the rig will be transmitting.

### Fixed

- **Nexus reopens at the size and the place you left it.** The main window went back to its stock
  1200×720, centred, on every single launch — resize it to suit your screen, quit, and the next
  start threw that away. It was most visible to operators running a manual UI scale on a 4K display,
  because the scale itself *was* being remembered: the app came back at 150% in a window sized for
  100%, the wrong shape for the setting it had just restored. The size was in fact being lost at
  every scale; the scale mismatch only made it obvious. Nexus now records the window's size and
  position when you close it and restores both next time, per radio, so two rigs do not fight over
  one size. Maximized stays maximized, and un-maximizing afterwards gives you back the size you had
  before rather than the whole screen; closing while minimized keeps your last real size rather than
  the minimized one. Nothing is replayed blindly: a window saved on a monitor you have since
  unplugged, or saved bigger than the display you are sitting at today, opens centred and clamped to
  the screen you actually have instead of off-screen or overhanging — the main window is the whole
  app, so stranding it where you cannot reach it would leave no way back. The box is applied before
  the window is drawn, so there is no open-then-jump.

  *(This entry was missing when 1.0.2 shipped — the fix was in the release, the note was not. The
  wording is Justin G0KSC's, from the parallel fix he sent as PR #11, written before either of us
  knew the other was on it.)*

- **An FT8 QSO between two callsigns the message format cannot carry together never finished — the
  two stations traded the same transmission back and forth until an operator gave up.** Nine of the
  sixteen combinations of callsign shapes could not complete a contact. The one most people meet is
  a home station working a DXpedition with a compound call such as `PJ4/K1ABC`; a `/P` station
  opposite a `/R` station, and either of those opposite any nonstandard call, are the rest.
  The 77-bit protocol has one message type that puts both callsigns on the air in full, and it has
  room for nothing else — no grid, no signal report. Its whole payload is a blank, `RRR`, `RR73` or
  `73`. Some pairs of callsigns force that type, because it is the only way to name both stations
  correctly, and once it is forced "I am calling you" and "here is your report" become the identical
  transmission. The auto-sequencer had no rule for that: each station read the other's report as a
  call, answered it with a report of its own, and then waited for a roger the other was equally
  waiting to receive. Nothing was malformed and nothing was mis-addressed — every transmission in
  the deadlock was a legal, correctly-addressed FT8 message to the right station — so nothing on
  screen said anything was wrong. The sequencer now separates the two the way the protocol intends,
  by what it last sent itself: if it is waiting for a roger, an over from the station it is working
  is that station's answer, and the exchange moves on. All sixteen combinations run to 73 in the
  usual five or six overs.

- **Those contacts also reach the log now, and the signal-report fields are left empty rather than
  filled with a number nobody sent.** Seven of the sixteen combinations exchange no numeric report
  in either direction — the messages have no field for one, so neither station can send it and
  WSJT-X cannot either. Auto-log required a report to have been exchanged, as its check against
  logging a contact that never happened, and for these pairs that required the impossible: the QSO
  completed on the air and was then dropped, the Log QSO button no-opped, and a CQ run stopped dead
  because it waits for the contact to be claimed before it calls again. The check against a phantom
  contact is intact — it now asks whether the station you were working ever actually answered you
  **and whether you ever transmitted to it**, which is what it was always after — so double-clicking
  a stray `RR73` still logs nothing.

- **A contact you never transmitted a single time could be written into your log.** This is the one
  worth reading twice, because a log that invents a contact is worse than a log that drops one: it
  goes to LoTW and QRZ under your call, the other operator never worked you, and nothing on your
  screen would ever have told you. Click a station to work it, let the transmission sit queued
  without ever keying — transmit off, a band change, a rig that never went to transmit, or simply
  not getting to it — and if that station's `RR73` to you was then decoded, the sequencer walked to
  the end of the exchange on *their* messages alone and the contact was logged. The Log QSO button
  had the same hole one step earlier: one over from the other station, no roger and no `73` from
  either side, and it would write the record. Both now require the same thing, in one place: that
  **you** put at least one transmission on the air in this QSO. That is the evidence the closing
  half of the sequence has always demanded, and the relaxation for the report-less callsign pairs
  above is what let the opening half go without it. Nothing that genuinely reached the air is
  refused — a real contact keys several times before it ends.

- **A `/P` or `/R` station could acknowledge a signal report nobody had sent it.** The rule that
  lets the report-less callsign pairs finish reads a bare "you, me" transmission from the station
  you are working as their answer, because for those pairs the message format has nothing else to
  put in it. It was reaching too far: it applied to any callsign containing a slash, and a portable
  or rover call working an ordinary station carries its grid and its numbers perfectly well. A plain
  station calling you with no locator set sends exactly that bare form — so you rogered a report
  that was never on the air, and the other operator's own sequence then had nothing to go on.
  The rule is now limited to the pairs whose transmissions genuinely have no room for a report;
  everywhere else a repeated call is treated as "they have not copied me yet" and your report goes
  out again, which is what it means.

- **A contact whose closing `73` never arrived was lost, and a CQ run stopped there.** In the
  callsign pairs above the roles of the exchange swap over: your roger is the transmission that
  ends the QSO, the other station takes it as the finish, logs you and signs off. Nexus was still
  treating that roger as the middle of the exchange, so it sat waiting for a confirmation the other
  operator had no reason to send, and if their parting `73` was lost to fading — which is precisely
  the transmission most likely to be lost, since nobody repeats it — the contact went unlogged, and
  a CQ run stopped calling because it waits for the contact to be claimed. The contact is now
  claimed off your own roger, once that roger has actually gone out, exactly as an ordinary FT8
  contact is claimed off your `RR73`; the run carries straight on to the next caller. The other
  station has logged you either way, so this is the log agreeing with theirs rather than a contact
  invented out of nothing.

- **Two stations that called each other at the same instant never got past exchanging grids.**
  Answer a station at the moment it answers you and both sides believed the other owed the first
  signal report, so both re-sent their grid, over after over, indefinitely — legal, correctly
  addressed transmissions that went nowhere. WSJT-X answers a grid addressed to it with a report
  whatever else it is doing, and Nexus does now too: whichever of you decodes first sends the
  report, the other rogers it, and the contact finishes in the ordinary five overs.

- **The SSTV mode picker understated every mode's airtime by about a second.** The pixel sizes were
  right, but the durations beside them were a hand-maintained copy that had drifted from what the
  encoder actually emits. They are now the exact figures, and a test compares the two tables so they
  cannot drift again — which matters more than it did, because the composer now uses that number to
  tell you how long the transmitter will be keyed.


### Fixed

- **Connecting to a serial rig could put it straight into transmit — and hold it there.**
  If your PTT Method is anything other than RTS or DTR on the CAT port — VOX and CAT keying, which
  is the default and what most operators run — Nexus started Hamlib's `rigctld` without telling it
  anything about the port's control lines. A serial port's driver raises RTS and DTR when the port
  is opened, and Hamlib only puts a line back down when it is the line it has been asked to key
  with, so on the default both stayed up for the whole session. On an interface wired to key the
  radio from RTS (or from DTR) that is the transmitter switched on by the act of connecting, with
  nothing in Nexus that would switch it off. Nexus now asks the daemon to hold those lines low when
  it opens the port, and closes the same gap in the two places it opens a serial port itself:
  serial PTT, which drove only the line it keys and left the other one up, and the native CI-V
  connection used by scope-capable Icoms, which keys nothing and so left both up. The CW and RTTY
  keylines already did this; the rule now lives in one place instead of being remembered at each.
  **Who was exposed:** a serial CAT connection through an interface that keys PTT or a CW line from
  RTS or DTR — the classic single-cable and homebrew interfaces, commercial CAT cables wired that
  way, and anything built to the RTS-is-PTT convention. **On every platform, not just Linux**: the
  earlier note here said Linux and probably macOS, and that was wrong about the platform most of
  you are on. Linux and macOS raise both pins on open. Windows raises them too, and while Hamlib
  lowers DTR itself as it configures the port, it leaves RTS to the driver — and RTS is the line
  the reports are about. A network/TCP rig has no control lines and was never affected.
  **What to check:** if you have ever seen the radio key on connect, drop to receive, or show TX
  with nothing sending, this was a candidate cause. **What has NOT changed:** if you key by RTS or
  DTR, Nexus never touches that line, so keying works exactly as before. Neither is RTS touched on
  a radio whose Hamlib backend uses it for hardware flow control (the FTDX10, FT-991, TS-2000 and
  TS-590 among about fifty others), nor is either line touched on a radio whose Hamlib backend keys
  the transmitter from that pin in its own right — the Yaesu FT-980 keys from RTS and the FT-757GXII
  from DTR, and Hamlib refuses to open either of them if it is also asked to hold that pin. Nexus
  asks the Hamlib you actually have, per radio, which lines it will accept before it starts the
  daemon, so this stays right when Hamlib changes: four popular radios gained the flow-control
  declaration between two Hamlib releases, and a list baked in here would have gone stale in the
  direction that costs you CAT. Anything it cannot establish, it leaves alone. If your interface
  takes its power from the DTR or RTS pin rather than from USB — a design from the RS-232 era — it
  will now see those pins low. **What this is not:** it is a real defect, found by reading the
  Hamlib source and confirmed against the Hamlib Nexus ships, and it matches a published report of
  the same behaviour with the same FT-847 cable under a different program. It is not established as
  the cause of any particular report of a silent radio, and it is not claimed as one — silence and
  a stuck transmitter are different symptoms. Nobody here has a serial rig: every Hamlib refusal
  above was reproduced by running the bundled `rigctld`, but no pin has been watched moving.

- **The Needed board hid US-spotted rows on exactly the pileups where a US spotter matters.**
  An HF spot reaches the board when someone on your continent heard the DX — a JA station a
  Kansas skimmer copied says something about a path from your QTH; the same station heard only
  in Europe and Japan does not. Nexus keeps a list of the other stations that reported each DX,
  capped at eight so a busy pileup cannot grow it without limit, and it was keeping the eight
  that arrived *first*. So the moment a ninth reporter came in, the newest voice was the one
  thrown away — and on a pileup being re-spotted from Europe and Asia several times a minute,
  the single North-American skimmer was gone within seconds of arriving. The board then saw no
  local report and dropped a row you asked to keep. The cap now keeps the reports that answer
  the question the board is asking — the ones near you first, then the ones on your continent —
  so one US skimmer is enough no matter how loud the rest of the world is.

- **Changing digital mode could also make the Needed board's own-radio rows lie about their age.**
  The same clock renumbering, one screen over. "Decoded by YOUR radio on this band" is the
  strongest row the board can show — it is your own receiver, not somebody else's report — and it
  is capped at two minutes old for exactly that reason. That cap was computed the unguarded way:
  a station decoded on FT4 at 17:00 and read after a switch to FT8 at 18:00 came out as zero
  seconds old, so an hour-old decode led the board as the freshest thing on the band. Both places
  that turn the heard-list's slot number into an age now go through the same conversion, and it
  refuses a number that belongs to a clock no longer running.

- **A 6 m spot's local reports were being thrown away in favour of ones the 6 m gate cannot use.**
  Nexus keeps up to eight of the other stations that reported each spot, and the entry above
  taught that cap to keep the reports nearest you first. It measured "near" once, at the widest
  radius any band uses — 800 km, which is the 2 m figure. On 6 m the board asks a tighter question:
  it wants two reports from inside 250 km, because that is the size of a sporadic-E patch. From
  EN52, sixteen skimmers publish a grid inside 800 km and only five inside 250 km, so the eight
  slots filled with reports that ranked as "near" while being no use to 6 m — and pushed out the
  ones that were. A genuinely corroborated 6 m opening then read as a single report and was
  dropped. The ranking now asks each band's own question at that band's own scale.

- **A short-skip sporadic-E opening on 6 m did not raise an alert.** The regional opening gate —
  the one that fires on a band-wide surge among the receivers around you — gained a distance test
  below, so that a busy evening of local 6 m FT8 could not pass for an opening. The test asks for
  a path past 500 km, but the near-receiver half of it was measuring 700 km. Sporadic-E on 6 m
  starts at about 500 km, and a burst that puts a dozen stations 540–580 km out through three
  receivers near you is a real opening and the one a 6 m operator most wants to hear about; it
  satisfied neither half and the gate stayed shut. The near-receiver test now measures at 500 km,
  where the physics puts the floor. Nothing else moved: the receive-only sentinel — the alert
  that fires when you are not even on the band — still needs two independent receivers past
  700 km, and the anti-superstation rule it exists for is unchanged. One near receiver is enough
  for the distance test alone because that test is not the anti-superstation check: a big station
  on a hill can inflate a count of stations, but it cannot make a path longer than it is, and the
  gate separately demands three distinct local receivers before it believes anything.

- **Changing digital mode could make a dead 6 m band alert again.** The opening detector times
  your own decodes by the slot clock, and that clock is renumbered whenever the transmit period
  changes — 15 seconds on FT8, 7.5 on FT4, 4 on FT1. Stations already in your heard list survive
  a mode change carrying their old numbering, and read against the new clock they could compute
  as "decoded this instant". An hour-old decode became the freshest evidence on the band, which
  is the same stale-evidence fault that made 6 m alert on nothing and stay latched. Entries
  numbered under a clock that no longer exists are now dropped rather than guessed at; anything
  still on the air is re-decoded within a slot or two.

- **6 m could still open on activity that never left the neighbourhood.** The tightening that
  removed distance-blind evidence from the opening gate missed one path: the regional gate, which
  fires on a band-wide surge near you and asked four questions — how many stations, how many
  local receivers, how many two-way contacts, how band-specific — without ever asking how far
  anything went. A busy evening of local 6 m FT8 among a dozen neighbours answers all four the
  same way a sporadic-E opening does. That gate now also requires at least one path past 500 km,
  either one of yours or one a nearby receiver copied. HF is untouched: an F2 opening's paths are
  continent-scale by definition, so a distance test there would only add noise.

- **The openings log under-counted the stations in a VHF opening.** Confining the 6 m/2 m open
  gate to the last ten minutes — so a ninety-minute-old decode can no longer prop a gate open —
  was also applied to the station census the display and the openings log show. A forty-minute
  Es episode with thirty distinct stations, never more than a handful in any one ten-minute
  window, was journalled as about a quarter of the stations it actually carried. The gate keeps
  its ten-minute window; the counts are the whole episode again.

- **Purging the logbook left the LoTW and eQSL sync positions behind, so the next sync brought
  back a sliver of your confirmations.** An operator purged a 26,000-QSO log, re-synced, and his
  awards card read "3% — 816 of 26,007 QSOs confirmed": DXCC 131 against LoTW's 249, the
  Challenge 277 against 1,202, WAZ 31 of 40, and 23 DXCC credits against the 248 ARRL had
  granted him. Every worked total was right and every confirmed total was wrong. Each service is
  synced incrementally — Nexus remembers the date of your last confirmation and asks the service
  only for what has been matched since — and purging the log did not clear that date. So the
  next "Sync LoTW now" asked for the last few days of matches against a log that had just been
  emptied, and brought back a few hundred confirmations instead of a whole history. That date
  gates the confirmation pull and nothing else, which is why the contact totals looked right
  while every award did not. Nothing but changing your LoTW username ever reset it, so the rest
  stayed out of reach permanently. A purge now clears both positions. **The purge dialog says so
  before you confirm**, because the sync that follows is a full confirmation-history download and
  takes considerably longer than a routine one — it is the pull that brings your confirmations
  back, not a hang.

- **Importing a confirmation report over the contacts it describes changed nothing.** A LoTW,
  Club Log or eQSL download restates contacts you have already logged — that is what a
  confirmation report *is* — and the confirmation, the granted award credit and the STATE and
  COUNTRY ride *on* those restated rows. **Import treated a row matching a logged contact as
  nothing but a duplicate and dropped the whole row**, so importing a download over the log it
  describes skipped every row as a dupe and repaired nothing. "Already in the log" means do not
  log it twice; it never meant ignore what the row says about the one you have. An import now
  merges those rows into the contacts they describe, using the same one-way merge the **Sync
  confirmations** button uses — it only ever adds, so no import can un-confirm or un-credit a
  contact, and a QSL you have merely *requested* still counts for nothing. The import toast
  reports updates separately from the dupe count, because an import that adds no contacts at all
  can be the one that repairs every award total you have. **This is the hand-repair path**: if
  your confirmations are behind, download your report from LoTW (or Club Log, or eQSL) and
  import it, and it lands on the contacts you already hold instead of being thrown away.

- **Logging a contact re-read your whole logbook off the disk.** Two copies of Nexus can share
  one `log.adi`, so before rewriting that file Nexus checks whether the other copy has written to
  it since — a cheap comparison of the file's timestamp and size against what Nexus last saw.
  Writing a contact onto the end moved both, and Nexus never updated its own note of them, so the
  check missed every time and re-parsed the entire file instead: measured at 60 ms on a
  26,007-QSO, 3.7 MB log, on every contact you log and on every contact WSJT-X hands over in
  companion mode. Nexus now notes what it wrote — but only when it can account for every byte in
  the file, so a contact written by the other copy still forces the re-read that keeps it from
  being overwritten.

- **A confirmation marked `V` read as unconfirmed.** ADIF spells a confirmation you hold two
  ways — `Y`, and `V` for one an award credit has been granted against — and Club Log and
  DXKeeper both write `V`. Nexus read only `Y`, so importing a master log from either of them
  silently dropped the confirmations on exactly your *best* contacts, the credited ones. Both
  are now read; `N`, `R` (requested, not received) and `I` still confirm nothing. A value padded
  with spaces by a sloppy export is no longer read as a refusal. This is separate from the two
  LoTW faults above and never touched a LoTW download: LoTW's own report only ever writes `Y` or
  `N`.

- **Light theme: the "NEW ONE — Work it" pounce banner had no readable text.** When a needed
  station appeared, the banner slid down from the top of the screen with its NEW chip and its
  Work it button — and nothing between them. The callsign, the country, the frequency, the mode
  and the band were all being drawn in the light theme's near-black ink on a near-black
  background, so the one thing the banner exists to tell you was the one thing you could not
  read. The chip and the button paint their own colours, which is why they still showed and the
  data did not. Dark mode was never affected. The self-update prompt at the bottom right had the
  same fault from the same cause and is fixed with it.

- **Light theme: the "needed" colours were the dark theme's colours everywhere they appeared.**
  The eleven colours that say what is worth working — magenta for an all-time new one, violet for
  a new zone, orange for a new band-slot, and the rest — had light-theme versions written for
  them, and none of them were ever used. The light theme rendered the dark theme's pastels
  instead: colours picked to glow against a near-black panel, shown on white. That affected the
  need chips on roster/station cards and in the Satellites pass list, the tags and row tints in
  the decode feed, the POTA/SOTA/DXpedition badges on the band strip and band map, and the mode
  cells on the Needed board — where the FT8, FT4 and RTTY cells were pale fills carrying
  near-white text, effectively blank. All of it now uses the light inks. **You will see these
  colours change** in light mode; they are the same eleven hues, taken dark enough to read on a
  white panel. Three of them (new US state, wanted, new mode) were re-tuned a shade darker than
  originally written, because this is the first time they have ever been rendered and measured,
  and they fell short of the 4.5:1 readability floor. The world map is not part of this: it
  carries its own copy of the palette and still draws the dark colours in light mode.

- **Light and dark: the ATNO, new-band and SOTA badges had unreadable labels.** The white letters
  on the magenta "NEW ONE" decode tag (3.4:1), on the orange band-slot tag (2.2:1) and on the
  violet SOTA badge (2.5:1) all sat below the 4.5:1 readability floor in dark mode. Every badge
  that fills itself with a needed-colour now takes its letter colour from the theme, so it stays
  legible in both. **You will see those three labels flip from white to near-black in dark mode.**

- The pounce banner's bottom corners are rounded again — the rule that rounded them named a
  design token that does not exist, so it had been doing nothing and the banner was shipping
  square-cornered.

- **A Hermes Lite 2 running Thetis could not get CAT working — and Nexus told its operator to
  check the radio was powered on while quoting that radio's own greeting back at them.** CAT
  worked in WSJT-X. It worked in Nexus too, but only after the operator discovered that
  picking a *FlexRadio* profile got it going, which is not a thing anyone should have to find
  out. Three faults, one report.

  **The rig list named no program.** For a Hermes Lite 2, an ANAN, a legacy Flex, the SDR
  console on your PC *is* the CAT port — the board on the desk has none of its own. The list
  named only hardware, so searching it for your radio turns up nothing, and the FlexRadio
  entries are the closest-looking thing there. They do connect. They also cost you the
  S-meter (Hamlib's FLEX-6000 profile carries no signal-strength reading at all) and send
  keying without the read-back the PowerSDR-family profiles use. The default rig list — the
  one you see without ticking "Show all models" — now carries **Thetis (Hermes Lite 2 / ANAN
  / HPSDR)**, **PowerSDR / mRX PS (Apache ANAN / legacy FLEX)**, **piHPSDR / OpenHPSDR (Hermes
  Lite 2 / ANAN)** and **SDR Console**, each named for the program you launched. PowerSDR is
  no longer filed under FlexRadio, and no longer described as a TS-2000 emulation, which it
  never was. These four are list entries only: a program is not a USB device, so Detect and
  the CAT port auto-test never suggest one and never replace the model you configured.

  **Nexus mistook a rig's CAT port for a rigctld.** Before starting its own copy of Hamlib's
  rigctld, Nexus checks whether one is already running so it can share it rather than fight
  for the radio. That check accepted *any* reply as proof — and Thetis greets every program
  that connects to it. So the greeting alone convinced Nexus a raw CAT port was a rigctld, and
  it spoke the wrong protocol at a perfectly healthy CAT server until the timeout ran out. The
  check now reads what came back. When the program announces itself, Nexus names it: *"…is
  Thetis's CAT server, not a rigctld"*, quotes the greeting it was sent, and names the profile
  written for that program. If you are on a FlexRadio profile it also spells out what that
  profile is costing you. **Nothing is changed for you** — the message says what to set.

  **Both ends of the chain on one port.** When Nexus starts its *own* rigctld, that daemon
  binds a port and then dials your rig — it cannot dial the port it is listening on, and
  nothing checked. Giving the rig's Network Address and the rigctld TCP Port the same number
  is now caught before that daemon is launched, with both numbers read back so you can see the
  collision instead of being told about it. It is checked at that moment and no earlier, on
  purpose: when a rigctld **you** started is already on that port — the setup the manual
  prescribes for a radio outside the rig list (**NET rigctl**, Network Address
  `127.0.0.1:4532`, rigctld TCP Port 4532) — the two ends genuinely are one endpoint, and
  Nexus shares it exactly as it always has.

  What this does *not* do: recognition is by greeting, and only Thetis and PowerSDR announce
  themselves. piHPSDR, SmartSDR CAT and rigctld itself all stay silent until spoken to, and
  Thetis's greeting can be switched off in its own settings — in every one of those cases
  Nexus reports what answered and quotes it, rather than guessing a program. No hardware is
  ever inferred from a greeting: the interesting-looking tail of a Thetis banner is a
  build label, not your radio.

  Two smaller things from the same report. The Connection row read "Network (FlexRadio /
  remote)", which tells an SDR operator the row is not for them; it now reads **"Network
  (host:port — SDR software, or a remote rig)"**, and the address field says where to read the
  real port out of your program instead of leaving you to guess (Thetis's TCP/IP CAT server is
  a different box from its TCI server, and only one of them is CAT). And the two FlexRadio
  native-stream toggles — SmartSDR panadapter and DAX audio — were offered to any rig whose
  model *name* contained "flex", which the PowerSDR entry's did; they now appear only for an
  actual FLEX-6000.

## [1.0.1] — 2026-08-05

### Added

- **Scroll any digit of the frequency dial to tune by that amount.** Hover the 100 Hz digit and one
  notch moves 100 Hz; hover the MHz digit and one notch moves a megahertz. It carries like a real
  VFO — 14.199 rolls up to 14.200 — and every digit is live, including MHz. The dial stops at the
  edge of the band rather than refusing to move, and it will not tune while the radio is
  transmitting. The tuning-step selector and the scope wheel are unchanged; this is in addition to
  them, not instead. On the main dial in Operate, Phone, CW, RTTY and SSTV.

- **4 m (70 MHz) is now a band you can pick in Settings ▸ Frequencies**, with FT8 at 70.154 MHz and
  the SSB/CW and AM/FM calling channels, alongside the MSK144, JT65 and WSPR channels already
  there. Frequencies are the IARU Region 1 plan. ⚠️ **4 m allocations vary a great deal by
  country** — several are narrower than 70.0–70.5 MHz, and there is no US allocation at all, so a
  US station will see the band but cannot transmit on it. Check your own licence. No FT4 channel is
  offered because no 4 m FT4 frequency exists in any band plan we could find.

### Fixed

- **The DXpedition board recommended 60 m for everything, on every card.** An operator with 60 m
  enabled saw all eleven WORK NOW cards read "Best shot: **60m**" — the 12 m card, the 20 m card,
  the 160 m card, all of them. The windows differed per expedition, so the path modelling was
  running; only the band was stuck. Three separate faults, now all fixed.
  **The model had no ceiling for 60 m.** Every band carries a "how hard is real DX here even when
  the band is open" cap; 160/80/40 m had one and 60 m had been missed, so it alone was allowed a
  perfect score. That matters at night, when the model's other discriminators go quiet and the cap
  becomes the whole of the ranking — 60 m scored an identical 0.939 on paths from 2,214 km to
  11,589 km and won them all. It now sits at 0.80, between 80 m and 40 m where it belongs, and a
  test refuses any future band added without a cap. The same fault was showing in the general band
  ladder in Connect, the globe and the map, where 60 m had been reading as the single best band for
  every operator on Earth at every hour of the day; 60 m no longer wins there either.
  **What is honestly not fixed:** that ladder still names one band far too often — 40 m now, where
  it used to be 60 m. The cause runs deeper than any one band and is described under *Known* below.
  40 m at least earns DXCC credit and is a band expeditions actually run, so the recommendation is
  no longer actively misleading, but it is not yet carrying much information.
  **A card now reports its own band.** Each card covers one band, but the "best shot" line was
  ranking every band on the path — so a 20 m card would headline 60 m, a band that expedition may
  never have announced. Each card now shows the window for the band it is actually about, and says
  so plainly when that band has no opening.
  **And nothing chasing DXCC leads with 60 m any more.** 60 m earns no ARRL DXCC credit of any
  kind — not Mixed, not per-band, not the Challenge — and most DXpeditions do not run it: it is
  secondary, channelised, and a single-channel receiver rules out the split that runs a pileup. So
  the headline on a chase now goes to the best band that can actually earn you credit. 60 m is
  demoted, never hidden: it keeps its true score and its real window right beside the headline, and
  if it is genuinely the only thing open it still leads and says so. Where the question is not
  chasing — "which bands are open right now", in Connect and on the map — 60 m is left exactly
  where the physics puts it, because there it is a perfectly good answer.

- **Linux: the audio device list finally reads like your own machine, and shows all of it.**
  An operator with eight sound cards — an FTDX10 among them — was offered one card, named
  `hw:CARD=Device,DEV=0`, while his system, `aplay -l`, KDE, pavucontrol and WSJT-X all called his
  radio `USB AUDIO CODEC`. Nothing in the list matched anything he could see anywhere else, and his
  log filled with ALSA errors. Two causes, both in the audio library Nexus used to enumerate
  devices: it named each device by its raw ALSA PCM string and threw the human description away,
  and it tried to *open* every device to see whether it existed — so anything held by PipeWire, or
  by another application, silently disappeared. Nexus now reads ALSA's own device list directly. It
  opens nothing, so a card that is momentarily busy is still offered; it uses ALSA's own
  input/output flags instead of guessing; and it shows one entry per card, under the name the rest
  of your desktop uses. The error flood is gone because nothing is being probed. **After updating,
  re-pick your audio devices once** — the list now prefers the format-converting entry for each
  card, which is the forgiving one for a radio codec, and your saved entry may be the older strict
  one. Your existing choice keeps working and stays selected until you change it.
- **Linux: Detect Rigs now fills in your radio's audio, which it never could before.** Automatic
  rig-audio pairing matches on names like "USB AUDIO CODEC" — and on Linux it was only ever shown
  `plughw:CARD=CODEC,DEV=0`, so it matched nothing and left audio blank on every Linux station
  since the feature shipped. It now sees the real card name. Where a machine has more than one
  generic USB audio device, the more specific match wins rather than whichever came first in the
  list, so a radio codec is preferred over a plain USB sound adapter (this part applies on Windows
  and macOS too).
- **A radio audio device that will not open now says so instead of quietly using the wrong one.**
  If you had explicitly chosen your rig's codec and it was unavailable — powered off, unplugged,
  or held by another program — Nexus fell back to the system default without a word. On a laptop
  that means receiving from the built-in microphone and sending transmit audio to the speakers,
  while PTT still keys the radio over CAT: a dead unmodulated carrier on the air that looks like
  everything is working. You now get a banner naming the device that failed. Nexus keeps running on
  the system default so the radio is not dead in the water, and **keeps retrying your chosen device
  every couple of seconds** — so a rig you switch on after Nexus, or a codec another program was
  holding for a moment, is picked up on its own within a few seconds and the banner clears itself.
  "System default" is still a real choice and still behaves exactly as before. If you have a device
  saved that is not one of the ones offered, the picker now says **saved, not in the list** rather
  than claiming it is absent — on Linux the list is pruned to one entry per sound card, so a device
  can be perfectly usable and still not be one we offer. Whether it actually opens is answered
  where it can be answered honestly: at open time, by name, in the banner.

- **FT8 decodes the station answering your CQ when you transmit off your receive frequency.**
  WSJT-X gives its deepest decoding two frequency windows, not one: around where you listen, and
  around where you transmit. The second exists because a caller normally answers on *your*
  transmit frequency, and with "Hold Tx Freq" on those are different places. Nexus only ever had
  the first, so the one signal in the slot you most want was the one decoded with the least help.
  Measured on a caller 900 Hz away from the receive marker: 12 of 12 recovered with the window,
  0 of 12 without. Nothing changes for operators who transmit and receive on the same frequency —
  the two windows land on top of each other, which is what WSJT-X computes too.
- **The FT8 early decode no longer costs twice what it should.** WSJT-X runs the peek at
  11.8 seconds deliberately cheap — a higher sync threshold and none of the a-priori passes —
  because the real decode re-reads the same audio at the end of the period anyway. Nexus was
  running the full-price decoder there. On the reference off-air recording that peek took 540 ms
  where it now takes 265 ms, and it returned the *same thirteen* signals either way. That matters
  beyond CPU: an early pass still running when the period ends makes the whole period's decode get
  dropped, which on a slower machine reads as a dead band. What the cheaper pass does give up is
  the a-priori recoveries — a station finishing a QSO with you, a few dB below the ordinary
  threshold, now arrives at the end of the period instead of three seconds early. The
  authoritative decode at the period boundary is unchanged, and it is the one the log and the
  roster are built from.

- **The FT waterfall.** It looked, in the operator's words, "so 8 bit" — and that turned out to be
  four separate causes stacked on one surface, all now fixed. The intensity axis was linear in
  *amplitude* against a reference that moved every 20 ms, which left the noise floor about 15 of
  256 shades to work with and reserved the whole colourful middle of the palette for signals that
  are rarely there; it is now dB against a fixed reference, and an ordinary run of FT8 signals is
  spread across the palette instead of crowded into one dark blue. Five of every six spectra were
  being thrown away rather than averaged, so the floor boiled frame to frame; each drawn row is
  now the mean of every frame since the last one. Bins were point-sampled onto pixels, painting
  each one as a hard rectangle, and the accumulated history repainted that way every time you
  touched the palette picker or resized a pane — both now interpolate. The FT waterfall also keeps
  its own palette, defaulting to Turbo, so a choice made in another mode no longer follows it.
  **What this does not do:** it does not reach WSJT-X's smoothness. Ours integrates ~171 ms per
  row against their ~2.5 s, which is the price of a display that updates 12× faster.
- The waterfall palette picker in Phone, CW, RTTY and SSTV said it applied to every mode. It no
  longer reaches the FT waterfall, and now says so.
- A capture underrun could blend one waterfall row across the gap, mixing audio from before the
  stall with audio from after it. One row in roughly 450, but it is now a clean restart.
- **MSK144 decodes about 90× faster** — 2.55 s down to 0.028 s for a 15-second period. The
  frequency tolerance was being derived from the receive passband, which made it 1350 Hz where
  WSJT-X uses 50; MSK144 sits at one fixed centre, so that width never meant anything. The same
  mistake let the decoder accept a signal a kilohertz off frequency, which is not an MSK144
  contact at all. This is the decode that could make the whole app appear to hang.
- **An FT1 over could run past the end of its own slot** and transmit into the period it should be
  receiving in. The transmit deadline had no slot bound, so any delay in keying the radio — a slow
  CAT link can spend a full second — pushed the end of the over out with it. WSJT-X clamps the
  window to the period and can never cross it; Nexus now does the same, for every mode. FT8 and
  FT4 were never affected in practice: they carry over two seconds of slack. FT1 carries none at
  all — its transmit buffer fills its whole 4-second period, so every FT1 over now ends exactly at
  the slot boundary. FT1's 4-second timing is unchanged. (An earlier draft of this entry said FT1
  had 214 milliseconds of slack; that was measured from the tones rather than the buffer actually
  sent, and was wrong.)
- **TempoFast (FT1) starts its transmission 100 milliseconds earlier in the slot**, which leaves
  164 milliseconds of quiet at the end of the over instead of 64. That trailing quiet is what
  protects the end of your signal: transmit stops at the slot boundary, and your sound card is
  always running a little behind, so if its buffer is longer than the quiet you have left, the
  last of your tones never reaches the air. Buffers of 20 to 100 milliseconds are completely
  ordinary, so the old margin was inside the range where that happens.
  **What it costs:** the lead-in is also TempoFast's tolerance for a station whose clock is FAST,
  and the two come out of the same fixed budget. A station more than 0.3 seconds ahead of UTC will
  now be missed where up to 0.4 seconds was tolerated before. Measured across signal levels, that
  tolerance is a cliff rather than a slope — inside it, decoding is unaffected — and the tolerance
  for a station running LATE improves by the same 100 milliseconds. If you work someone whose PC
  clock is badly fast, this is the trade you are on the wrong side of; the measurement that chose
  it is kept in the source so it can be revisited.
- **The QRZ link on the callsign card** only appeared once a callbook lookup had already
  succeeded, which withheld it in exactly the cases you want it — no QRZ subscription, no
  credentials, a lookup that failed, or a callsign QRZ has never heard of. Opening the page only
  ever needed the callsign. Separately, if the browser refused to open, nothing said so anywhere;
  every QRZ and DXpedition link now reports the failure instead of doing nothing.
- **SSTV gained the radio controls the other cockpits already had** — power, tune and stop from
  the header. Drive matters more here than anywhere else, because the picture rides in the audio
  and pushing past ALC visibly wrecks the image at the far end. The drop-image box no longer
  reserves 160 pixels before you have loaded anything.

### Known

- **The band recommender still favours one band too strongly.** After the 60 m fix above it usually
  says 40 m. The reason is in the model rather than in any band's settings: at night the prediction
  for every band below about 9.6 MHz collapses onto a fixed per-band ceiling, so the 24-hour "best"
  figure the recommendation uses is a constant rather than something that varies with the path. Two
  paths 5,000 km apart can get the same answer. Fixing it properly means reworking how the night-time
  prediction is calculated — a bigger change with its own testing, deliberately not folded into this
  release. **What this does and does not affect:** the per-band cards, their colours and their sort
  order are computed from the CURRENT hour and are unaffected — they were always sound. It is the
  single "best shot" summary line that is worth less than it looks.

## [1.0.0] — 2026-08-04

Nexus has been a beta since its first public build, and this release closes the beta period.
What that means here is narrow, and worth saying plainly: the modes, the rig control, the logbook
and the awards engine have been run on the air through that period, on more than one station and
on rigs the author does not own, and what came back from that is what the last several releases
have been made of. It is not a claim that nothing is left — every entry below still says what a
change does **not** do, and that habit does not stop at 1.0. Windows, Linux and Raspberry Pi
build from the same tree and ship together, as they have every release. Upgrading asks nothing of
you: 1.0.0 installs over 0.27.0 and reads your existing log, settings and layouts as they are.

0.28.0, 0.28.1, 0.28.2 and 0.29.0 were bench builds and were never published. Coming from 0.27.0,
which is the last public release, this install brings you the 0.28.0 and 0.28.2 sections below as
well as this one.

### Fixed: two controls you could not reach — the Logbook's Log button and Program's export row

Both at 1024×768, the supported floor, and both with nothing to scroll:

- **The Logbook's manual entry form.** Press **Log QSO**, fill it in, and at a UI zoom of 115 %
  or more the **Log** button that commits it was off the bottom of the screen. No scrollbar,
  nothing to drag. The header above the form and the form itself are both fixed height, so the
  overflow had nowhere to go.
- **Program's whole delivery row** — **Export for CHIRP…**, **Export CSV**, **Save to Memory
  Bank**, **Clear**. At 120 % and up all four were off the bottom, sitting under two lists that
  each hold a floor of their own. That is the worse of the two: building a channel list and
  getting it out to the radio *is* the section, so the step you could not take was the last one.

One cause, and it is three rules deep. The view's wrapper carries a scrollbar, and the panel
inside it is told to be exactly the wrapper's height — so the wrapper's contents can never be
taller than the wrapper, and that scrollbar can never appear. Not because there was nothing to
scroll: **by construction**. The panel then clipped whatever its own column could not shrink,
which is how a button ends up painted nowhere.

Fixed at the rule rather than at the two symptoms. The panel keeps its exact height, because that
is what bounds the scrolling lists inside it — letting it grow instead would break every view
that works today: Settings would become one endless page, and the Logbook would stop virtualising
and mount every row in your log. The panel owns its own overflow instead. Five views had each
escaped that rule privately, one at a time, which is the tell that the rule was wrong rather than
the views; POTA's private escape is dead now and is deleted with the cause.

**This is a scrollbar, not more room.** At 1024×768 with a large zoom the open form is still
taller than the pane holding it — what changed is that you reach the button by scrolling instead
of not at all. Nothing moves at any size where it already fit.

### Fixed: Phone and CW stop cutting controls off at the window Nexus opens at

Reported as Phone and CW looking bad at low resolution, and the diagnosis is upside down from the
obvious one. At 1024×768 the panes run in a single column and that column has always scrolled —
the floor was the safe case. The clip bit at the **default window**, where the panes run two
columns and the region holding them does not scroll.

A column of control strips is a stack of things that cannot shrink, standing against an edge that
cuts, with nothing in between. Phone's leading column is control strips end to end, so the NR
slider, the AGC chips, the DSP toggles and the voice keyer's F4–F6 rendered past that edge — not
below the fold, *gone*, with no scrollbar and nothing to drag. The column scrolls now. In the
single-column layout the rule is inert, because a column there is exactly as tall as what is in it.

Two more, behind the same door:

- **Every pane that fills had a floor of zero.** CW's six aux strips cannot shrink, so they took
  their height out of the one pane that can — the decode transcript, the pane the cockpit exists
  for — and it could be starved toward nothing. There is a floor under it now, and it is written
  to yield: stated in text units rather than pixels, and never claiming more than the space it is
  in.
- **The column count was being read as a claim about width.** Untick CW's decode, sent echo and
  aux panes on a 3440-wide window and the log pane went content-height with the whole rest of the
  window left blank — because the ⊞ Panels menu could drive the region to one column at *any*
  width, and one column meant content-height rows. The track count and the width claim are
  separate things now, and it is the width that decides the vertical behaviour.

CW's leading column is no longer drawn when there would be nothing in it. One column used to mean
content-height rows, so an empty one was merely cheap; with the fill behaviour above it would
have been a strip of dead space beside the log.

All of this is layout. No pane changes parents, so a half-typed log entry and a recording in
progress survive every flip between one, two and three columns. PTT, Stop TX and Tune are not in
the pane region and are untouched.

### Changed: the CW decode window is the pane the CW cockpit gives room to

The decode transcript is the pane a CW operator actually works from, and it is declared the
primary one — but at the window Nexus opens at it was showing **three lines**. The reason is
arithmetic rather than a bug: at that width the cockpit runs two columns, so the Sent echo, the
scope controls, the DSP toggles, the RX DSP levels, Band Activity and the Copilot all share the
decode's column, and none of them can shrink. The transcript is the only pane that can, so it paid
for all of them and sat on its floor while the column scrolled 191 px past the bottom.

Four boxes that said what the pane frame already says once are gone, and **no type was made
smaller** — no font size, no density setting, no UI zoom:

- The **Sent** echo's own header, one label reading "SENT ▲" two lines under a frame head reading
  **SENT**. The blue stripe down its left edge is what tells you at a glance that these are your
  own transmissions, and that stays. Deleting the header is what let the strip come down from six
  lines' worth of reserved height to four, showing the same number of lines of text as before.
- The **Decode** pane's own header row. Its first word was "DECODE", under a frame head reading
  **DECODE**. Everything else on that row — the AI badge, the words-per-minute readout, the AI
  decoder switch, its status and **Clear** — moved up into the frame's own title bar, which had
  been rendering empty on every pane in this cockpit. Same controls, same order, one less row
  between you and the text.
- **Two of the three rig-control panes.** Scope controls, DSP toggles and RX DSP levels were three
  separate framed cards — three title bars, three sets of padding, three borders and three gaps —
  wrapped around one row of buttons each. They are now one **Rig controls** card holding one
  wrapping strip, and at the default width they sit on a single row. The ⊞ Panels menu is
  unchanged: all three are still separate entries, each still switching its own group on and off,
  each still explaining itself when your radio cannot feed it.

Result at the default window: the unshrinkable stack in the decode's column falls from 513 px to
406 px, what is left to scroll past falls from 191 px to 84 px — one short drag instead of two
screens — and the transcript goes from three lines to four. On a wide window, where the aux panes
get a column of their own, the decode keeps the whole gain.

In the header, the four keyer back-end buttons — CAT, Serial, WinKeyer, Soundcard — become one
dropdown, about 110 px wide where they stood nearly 300. That header also carries the band picker,
the tuning strip, Tune, Stop TX, speed, pitch, macros, filter width, memories and the rotator, and
on a 1024-wide screen its width is what wraps it onto extra rows. **Every word of the four
explanations survives**: each is on its own menu entry, and the one for the back-end you are
actually keying with is on the dropdown itself — including the soundcard warning about routing
audio to the rig and keeping drive below ALC, which is the one that matters on the air.

Stop TX, Tune and Esc are untouched: no control moved into a pane that the ⊞ menu can hide, and
none changed what it stops.

**Not done, deliberately.** Band Activity and the Copilot stay in the decode's column. Moving them
next to the log form is the only change that would close the remaining 84 px, and the log column has
no room to give — its own Log button sits 379 px inside a 392 px pane, so anything added above it
puts the button back below the fold, which is the defect the previous change was written to fix.

### Changed: the Phone cockpit's control column stops scrolling its own frame

At low resolution the left-hand column of the Phone cockpit — Band Activity, the voice keyer and
the rig-scope / DSP / RX-DSP strips — stands taller than the space it has, so the DSP buttons sit
below the fold and you scroll a *control* column in the middle of a contact. Nothing was
unreachable; it was simply further away than it needed to be.

Two of those panes were drawing a second card inside the pane's card, and one was printing a
paragraph that no longer needed a line of its own. Both are gone: the band strip's own border,
padding and margin, and the voice keyer's. That is 60 px, plus 41 px for the keyer's note, out of a
column that was over-full by 237 px at the window Nexus opens at — a bit over 40 % of the overflow,
recovered without moving a single control and **without making any type smaller**.

The band strip also stops printing "Band activity" one line under a pane head that already reads
**BAND ACTIVITY**. The live spot count beside it is not the same thing and stays. This lands in the
CW cockpit too, which frames the same strip.

What the keyer's note said is not chrome and has not been dropped: **● records from your input
device — often the rig's RX audio, not a mic** is the sentence that stops a slot going on the air
with the wrong audio in it. It now rides both controls that start a recording — the ● button and an
empty slot itself — so it reaches you where you are standing when it matters.

In the header, three words that repeated the control beside them are gone: **Record QSO** next to
the ● that records, **RX** next to a meter already announced as "RX audio level", and **Colors**
next to a palette picker already announced as "Waterfall color palette". Every one of them keeps
its name to a screen reader; what went was the printed duplicate, and with it about 120 px of a
header row that wraps at small windows.

### Changed: the log strip commits above the fold

Reported at low resolution: "the Log button ends up below the fold with a half-typed QSO above
it." Measured at the window Nexus opens at (1200×720), the strip stood 422 px inside a 392 px log
pane — the **Log** button 30 px past the bottom edge, so committing a contact meant scrolling to
find the button that commits it.

Two boxes above it were saying what the pane already said. The pane's own head reads **LOG** and is
its name to a screen reader; the strip printed "Log this QSO" again two lines below it. And the
strip drew its own bordered, padded card *inside* the pane's card. Both are gone from the Phone and
CW cockpits: 43 px off the top of the commit row, 56 px off the strip's height, and the button is
inside the pane at every window from 1024×768 up. **No type is smaller** — the space came out of
duplication, not out of the font.

The Satellites section keeps its heading. It hosts the same strip with no title above it, so there
the heading is the only thing naming the surface — this is a per-host decision, not a deletion.

Also in the Phone cockpit: the hint line under **PUSH TO TALK** is gone from the pinned transmit
dock, where it held a full line at every window size and repeated the button's own tooltip. The
part that was not a repeat — *you talk on the rig's mic* — is now on the button itself.

### Fixed: a cockpit header can no longer be crushed under its own controls

The header every cockpit shares carried a hard 44-pixel floor. A floor written that way forfeits
the automatic protection that stops a flex child shrinking below its own contents, so the header
was the one child of a cockpit column allowed to be squeezed smaller than what it holds.

**That shipped once, and it is worth naming.** At 1366×768 the CW header wrapped onto extra rows,
was squeezed back to 44 px, and drew those rows outside its own border box — where the scope
panel below it, which is opaque, painted straight over them. What went under the paint was the
keyer's **Speed** slider and **Tune / Stop TX / CAT**. A control that stops a transmission, under
opaque paint, on a screen that looked fine.

The repair at the time was a hand-maintained list of the four cockpits it had been seen in — CW,
Phone, RTTY and SSTV. That made the protection opt-in per cockpit, could not reach a grid-based
host at all, and left the Operating screen out; Operate survived on an accident of what it
happens to render today, which was asserted nowhere.

The 44 px is a real requirement — it is what keeps the header the same height across modes, so it
does not jog when you switch — and it stays. It moved onto the identity block the header always
draws, so it arrives as *content*: it raises the header's height instead of overriding the
header's minimum. The protection is back in every cockpit, including one nobody has written yet,
and the allowlist is deleted along with the reason for it. Nothing moves on screen, and Stop TX
and Tune stay where they are, outside every removable pane.

### Fixed: the APRS header stops cutting off its own controls

APRS was the one full view in Nexus with no way to scroll an overflow at all, and the reason
nothing caught it is that nothing ever checked — the census that computes this for the other
cockpits did not have APRS in it. Both are fixed.

The APRS header is a control strip that wraps: frequency picker, **Re-tune**, the TX arm latch,
**Monitor**, both health chips and the internet control. Narrow the window, or raise UI zoom, and
it grows onto more rows and cannot shrink below them. It sat directly against a hard edge — the
only other thing in the view absorbs none of an overflow — so everything the strip could not fit
was cut off with no scrollbar anywhere in the chain. The internet-feed panel is inside that same
edge: at 1024×768 under a pinned zoom the header wraps deep enough that the panel's Watched calls
row and the note under it fell past the bottom of the view, with no way to reach either.

The view scrolls vertically now. Sideways it still does not, on purpose — the station table
scrolls itself — and the view keeps the exact height the map canvas is drawn against, so the map
does not start growing again.

APRS has no removable panes, so nothing here was ever hidden by a ⊞ menu. The arm latch that
holds the transmit queue was simply out of reach at those sizes, and is not now.

### Fixed: the memory strip stops clipping its chips

The favourites strip in the Phone, CW and Operating headers carried a hard 26-pixel ceiling with
its vertical axis clipped — no scrollbar, no recovery. What sits under that ceiling is written in
your type, not in pixels: the chips and the **+** and **=** buttons are sized against the
browser's root font, and the strip reserves a horizontal scrollbar under them. At the ordinary
16-pixel root font one row already comes to about 29 px against a 26 px ceiling, so the bottom of
every chip and of both buttons was outside the clip **before** any UI zoom, any font bump, or any
theme with a thicker border — on an axis that cuts rather than scrolls. There was no way to reach
it.

Re-tuning the number is not the fix. A pixel ceiling over type-sized content is wrong in kind:
the two sides are denominated in different units and move on different inputs, so no constant
makes the relationship hold.

The ceiling is deleted, and the thing it was written to prevent — the old memory list growing the
top bar with every save — is held by the shape that already holds it. The strip is exactly one
row however many favourites you keep, bounded sideways, and scrolling horizontally past that;
scrolling surplus chips sideways is the right behaviour for a chip strip and is what makes one
row enough. The header does not get taller either: its cross-mode height floor is 44 px, well
above the strip's row.

### Fixed: opening Bonuses no longer flattens the Field Day sections board

The Field Day column is the banner, the header, the operator row, the score tiles, the sections
board, the **Bonuses** list and the log — and every one of them is a fixed height except the
board. So the board was the only thing that could give, and it gave for all of them. Open the
fifteen-row Bonuses list at 1200×750 and the board a club watches all weekend went to a blank
strip, with the growth past that off the bottom of the log and no scrollbar in the chain.

The cap on the Bonuses list was written as the fix for exactly this, and carried a comment saying
so. It could never have been: the cap is a fraction of the same window the space is coming out
of, so it bounds the list's own height and says nothing whatever about that height displacing its
neighbours. **The cap stays, and its comment now says what it does** — it gives you all fifteen
rows inside a list of its own, instead of about 290 px of checkboxes between you and the log.

Two rules do the actual work, and neither is any use alone. The board carries a floor, on the box
the outer column actually sizes — the old floor sat one level in, where the column could not see
it, and could not shrink, so a squeezed board just painted through the section beneath it. And
the column scrolls past that floor, because a floor with nothing above it turns a crush into a
clip, which is worse.

### Fixed: the scope splitter has travel at both ends, and RTTY's waterfall yields on a short window

Three faces of one defect: pixel limits written onto strips whose height is a share of the window.

- **The scope splitter's drag was dead at both ends.** Phone and CW handed the drag a minimum of
  100 and 90 pixels against a stylesheet floor of 112, and both handed it a maximum of 420
  against a ceiling that is 45 % of the effective window — 346 px on a 768-tall screen, 648 px on
  a 1440-tall one. So at both ends the pointer moved, the number moved, and the panel did not.
  The drag takes its limits from the same place the panel takes its size now, so the whole travel
  is live. Nothing renders at a different height on a 768-tall window; only the ends of the drag
  become real.
- **RTTY's waterfall had a 120-pixel floor and no way out of it.** RTTY is the one scope in Nexus
  with no splitter for you to drag, so that floor was unrecoverable: on a 768-tall window at
  175 % UI zoom the effective viewport is 439 px and at 200 % it is 384, and a large part of that
  was a floor you could not move. It yields now, and never claims more than 28 % of the window.
  Its 220-pixel ceiling went with it — that one bit only above a roughly 1000-pixel window, where
  it froze a tall display below the share every other size already gets.
- **The Phone scope panel's base rule carried a dead floor and a dead ceiling.** Both shipped
  hosts override them, so nothing was wrong on screen. But a dead rule holding a pixel floor
  *and* a pixel ceiling is a loaded gun: a third host rendering that strip would have been
  silently pinned between 120 and 220 px at every window and every zoom. Size belongs to the host
  now, and the base rule keeps chrome only.

RTTY's shorter floor only frees room above its transmit dock. Nothing else on that screen moves.

## [0.28.2] — 2026-08-03

### Changed: the Satellites section is a pass console

Reported after a pass worked with the manual rotor: "the schedule next list is long, and that
should be made smaller and scrollable to free up more real estate… I need to see the top next and
best 24 hours… my screen with my LOS and AOS… It also should contain the qso logging area along
with the frequencies and slections of what to select as a prominent feature… that main window
should have all contained, without any scrolling for normal operation."

It does now, and here is exactly where — measured, not rounded, because a promise that holds
nowhere is worse than a bound that holds. "Everything for the pass" means the sky dome and its
rise/set readout, the ground-track globe, the pass timeline, the log strip down to its **Log**
button, and the honesty line under it.

- **From a 1200×750 window upward — every size a Nexus window normally opens at — the whole set is
  on screen at once.** The only thing below the fold is the Birds catalog you use between passes,
  which is where this column deliberately parks its spare room.
- **At 1024×768, the smallest supported window, it does not quite fit.** The **Log** button sits
  6 px under the fold and the note under it 36 px: one nudge of the wheel, on a column that already
  scrolls to reach the Birds list. Nothing is unreachable and nothing is hidden behind a second
  scrollbar. And shrinking the sky dome would not buy it — going back to the smaller dome returns
  27 px and still leaves the last line 9 px under the fold. What would actually close it is
  dropping two of {that note, the strip's "Log this QSO" heading, the pass timeline}, and none of
  those is being taken away without asking.
- Also at 1024×768: the schedule table is wider than its column and scrolls sideways inside its own
  box. It stops doing that at 1366 wide.

What moved, and why it fits when it did not before. The detail column used to hold the bird's
name, four bordered instrument boxes, the sky dome, the pass timeline, the Doppler readout, the
passband, the log strip, the transponder cards and the globe — one on top of another, about three
screens of content in one column, with the ground-track globe last. Four changes:

- **The sky dome and the ground-track globe sit side by side.** The globe was always in the
  section; it was just two screens down. Stacked, the two square graphics were taller than the
  column they lived in at every window size. Abreast, and both capped against the window height,
  they cost the height of one.
- **The frequencies and the transponder chooser moved to the bottom left**, permanently on screen
  and never scrolled. Choosing a transponder is the most consequential control in the section — a
  wrong pick is a wrong uplink — and it used to be a long way down a scrolling column.
- **The bird's identity, the radio binding, Lock on and the five readiness gates merged into one
  arm bar** across the top, in place of six separately-framed boxes and a heading.
- **The schedule scrolls inside its own box** and is bounded by the frequencies panel beneath it
  rather than by any fixed cap, so the window decides how many rows you get. Counted on a 42-pass
  fixture, mid-pass: **5 rows at 1024×768, 7–8 at 1366×768 and 1280×800, 16 at 1920×1080, 24 in a
  tall 1200×1390 window and 30 at 3440×1440.** The "first 10 lines" you asked for arrives at about
  900 px of window height. Nothing was removed from the schedule — every pass is still there,
  below the ones you can see.

Nothing was deleted to make room and no control changed what it does — the density came from
merging frames and laying stacks out sideways, the same way the FT8 decode window was rebuilt.

### Changed: the sky dome, and the numbers on it, are smaller

"The actual aos, los and az, el text could be made smaller by 25%" — and, in the same breath, the
dome itself "could overall be reduced in size". Both, and they pull the same lever: the AOS/LOS
bearings and the live az/el are drawn on the dome, so their size follows its size.

At the 1024×768 minimum the dome used to render 458 px across, which made those numbers the
largest type anywhere in the section — bigger than the section heading. The dome is capped against
the window height now, and the plate text was scaled up against it so it does not shrink with the
dome by default. **The cut lands on the quarter you asked for: 24.8% at the smallest supported
window**, and more than that on bigger screens, where the old dome kept growing with the column
and the capped one does not.

One thing worth knowing before you reach for the zoom control: on a normal-width window the dome
is sized against the window's height, and that measurement does not change when you raise UI zoom.
So at a high zoom everything grows except the dome and the az/el numbers on it. On a narrow window
— or a wide one at a large zoom, where the layout goes to one column — that is fixed: the dome is
sized in text units there and grows with the rest of the interface.

### Added: the grid square is a field you can type in the Satellites section

The satellite log strip now has a **Grid** box, beside the two signal reports. The square was
already in the record and already on the callsign card, but only a callbook could ever write it,
so a locator a station passed you **on the air** could not be entered at all — and on a bird that
is most of the contact. Satellite work is grid-for-grid, and the grid is what Satellite VUCC is
scored on.

It sits with the reports rather than down with QTH because that is what it is on a satellite:
something the other operator says in the same breath as his report, read and typed in the same
moment. It also costs the strip nothing there — that row already wraps, and the box lands in the
space the wrap leaves. **The whole strip is 48 px shorter than before** across the ordinary range
of window widths: the park row below came out and nothing took its place.

One honest exception, because it is measurable and somebody will meet it: in a narrow band of
window widths the Grid box does not fit the space the wrap leaves and takes a line of its own,
and there the strip is 4 px *taller* than before rather than 48 px shorter. It is a band, not a
threshold — wider and narrower windows both get the full saving.

The callbook still fills the box when it is blank, and it will **not** overwrite a square you
typed — and a callbook answer that is not a locator at all (a rover's `EN52/EN53`, free text)
does not go into the box, because an empty box logs an empty grid. Nexus takes a **4-, 6- or
8-character** locator (`EN52`, `EN52XA`, `EN52XA25`) — every length ADIF carries, so every one of
them uploads. Anything else holds the **Log** button until you fix it or clear it, and says so on
the line above the button. A blank grid is not an error — most contacts have none.

**Not in the Phone and CW strips yet**, and that is a height decision rather than a verdict: the
box costs each of those a wrapped line, and this change was asked to stay on the satellite
section. The case for putting it there is good and still open — a wrong callbook square cannot be
corrected in those cockpits today, and they are the ones that meet the rovers and portable
stations whose square is wrong most often (2 m and 6 m and the VHF contests are grid-for-grid too).

### Changed: a bird's exchange has no park in it

There is no park on a satellite. The section asks the shared log strip for a **satellite**
exchange, so the program picker and the park search are not built there at all, and the sky dome
has that room instead. The Phone and CW strips are unchanged: hunting an activator is part of the
exchange there, and the park row stays.

No park reference reaches a satellite contact by any other route either. A hunt you started in
another section leaves a pending reference behind, and the satellite exchange refuses it whatever
that hunt is doing — so a pass cannot put a park contact in your log with no row on screen to
show it or clear it.

## [0.28.0] — 2026-08-03

### Added: log a contact from the Satellites section

Reported after a clean pass: "the doppler shift change are working amazing, I was following the
bird with my manual rotor and it was perfect. The problem came when I tried to log someone, as I
dont have a spot to log within the satellites section to log my sat qso's."

There is now a log strip in the bird's detail column, directly under the Doppler readout. It is
not a new form — it is the same log strip the Phone and CW cockpits use, with the same callbook
lookup, the same recall card and the same prior-contact history, put where you are working the
pass. It sits above the transponder cards and the globe on purpose: with both hands on a rotator
and seconds between overs, a form at the bottom of the column is a form you don't use. It is
there whether or not a pass is armed, and it stays after the bird sets, so you can catch up on a
contact once your hands are free.

**It logs an ordinary contact.** The call, your dial, your band, the mode you are on, the time —
exactly what the Phone strip logs from the same state. It does **not** tag the contact as a
satellite QSO: LoTW wants the ADIF `PROP_MODE` and `SAT_NAME` fields for that, and Nexus does not
write them. The strip says so under the fields, so nobody waits on satellite credit that isn't
coming. If you want it, add **both** fields yourself in whatever you upload from — LoTW turns away
a contact carrying only one of the pair, so half the tag is worse than none.

**Three things it does not do yet.** None of them is a decision that satellite work should stay
this way — they are the price of dropping the existing strip in unchanged instead of building a
satellite-aware one, and each is meant to be closed.

- **Your satellite grids land in the wrong place, in Nexus and at ARRL.** Nexus decides "was this a
  satellite QSO?" from `PROP_MODE=SAT` alone; LoTW wants that *and* the satellite's name. With
  neither written, a contact logged here counts toward neither the **Satellite VUCC** totals on the
  Awards screen nor the satellite needs board — and its grid does not simply go uncounted. Nexus
  keeps per-band grid counts for 160 m through 2 m only, so anywhere outside that range — 2200 m
  and 630 m below it, and 1.25 m, 70 cm, 23 cm, 13 cm, 9 cm, 5 cm, 3 cm and 1.2 cm above it — the
  grid lands nowhere at all. On the bands that do have a slot — for satellite work that means
  **2 m**, the downlink of every U/V bird, and **10 m**, where AO-7's mode A comes down — it lands
  in your **terrestrial** VUCC count for that band, which is a grid ARRL's rules say a satellite QSO
  does not earn. LoTW files the upload the same way. If you chase VUCC, add both fields by hand
  before you sign.
- **During Field Day it logs to the general log, not the contest log.** The Phone and CW strips
  switch to the Field Day log while a session runs; this one is not wired to Field Day yet, so a
  satellite contact made during FD scores the club nothing. Log those from the Phone or CW
  cockpit for the duration — adding them to the FD log later files them on the band you are on
  then, not the band you worked.
- **The recorded mode comes from your sideband**, so it names a voice mode when you are on a data
  mode: `SSB` on the WSJT-X tiers (every channel there commands USB), `FM` on Tempo's three FM
  simplex channels. Right for voice and CW, wrong on a data mode. **Log a contact from another
  radio** lets you set the mode by hand, but its picker offers SSB / FM / AM / CW / RTTY / FT8 /
  FT4 and nothing else — so of the data tiers it covers FT8 and FT4 only. On Q65, JT65, MSK144,
  WSPR, FST4 or Tempo, log the contact and then correct its **Mode** in the Logbook, whose mode
  field takes any text.

All three are written up in the guide's [satellite chapter](docs/guide/satellites.md).

### Fixed: contacts logged during a pass no longer carry a satellite name LoTW rejects

This one is in the version you are running now. Since 0.24.0, any contact logged while you had a
transponder held was written with `PROP_MODE=SAT` and a `SAT_NAME` — and the name was the bird's
*catalog* name, "SAUDISAT 1C (SO-50)" rather than "SO-50". LoTW rejects a record naming a
satellite it doesn't recognise (ARRL: "if you enter the satellite name as AO7 instead of AO-7 the
data will be rejected") — the signing tool won't even sign it — so those contacts could never earn
satellite credit, and the batch each one rode in came back marked **Rejected**.

It also caught contacts that had nothing to do with a satellite. The hold is only handed back when
the pass ends — and a transponder you pick without arming a pass is never handed back at all — so
an HF contact made an hour later got tagged as a satellite QSO too, and went out that way to LoTW,
eQSL, ClubLog, QRZ Logbook and Cloudlog.

Nexus no longer writes either field for any contact it logs. Records that arrive carrying them —
a foreign ADIF import, or one you fixed by hand — are untouched, on import, on export and in the
logbook.

**That has a cost inside Nexus too, and it is not permanent.** The same `PROP_MODE=SAT` field is
what Nexus reads to decide a contact was a satellite QSO, so with nothing writing it your
**Satellite VUCC** totals and the satellite needs board no longer see contacts Nexus logs for you
either — and on a band Nexus keeps a per-band grid count for (2 m most commonly, since it is every
U/V bird's downlink) the grid is counted toward your terrestrial VUCC for that band instead, which
is not a grid a satellite QSO earns. Writing a satellite name Nexus can stand behind is work that
has not been done yet, not work that was ruled out.

**Writing just `PROP_MODE=SAT` is not a shortcut** — that was looked at and rejected, not
overlooked. TQSL refuses to sign a contact whose propagation mode is `SAT` when it names no
satellite ("PROP_MODE = 'SAT' but no SAT_NAME"), exactly as it refuses a name it doesn't
recognise. Half the tag costs you the whole QSO at LoTW — including the DXCC and WAS credit an
untagged upload *does* earn — and it takes the upload with it: Nexus asks TQSL to skip bad records
rather than abort, so the batch signs without that one and comes back marked **Rejected**, and it
will keep coming back that way every time you upload while the record is in your log. Until Nexus
can write a name it can stand behind it writes neither, and adding **both** fields by hand restores
everything: the LoTW credit on your next upload, and the in-app totals the next time Nexus starts
and re-reads the log file.

**Contacts already logged that way are left alone**, and deliberately: some of them were real
satellite QSOs that want the name corrected, some were ordinary contacts that want the tag gone,
and nothing in the record tells them apart — that call is yours. To find them, search your log
file (`~/.config/tempo/log.adi`, or `%APPDATA%\tempo\log.adi` on Windows) for `SAT_NAME`, and edit
it there with Nexus closed. The guide's [satellite chapter](docs/guide/satellites.md) walks
through it.

### Fixed: the microwave bands go out to N1MM / N3FJP in metres

The `band` field on the club-log wire is a **metre count** — "20", "0.7". Nexus converted 70 cm,
33 cm and 23 cm by hand and guessed at everything else by chopping the letters off the end, which
cannot tell centimetres from metres. So every other centimetre band left as a bare number: a
13 cm contact was broadcast as "13", a 3 cm contact as "3".

Those are not hypothetical bands. The Q65 band plan ships 13 cm, 9 cm, 5 cm, 3 cm and 1.2 cm
channels (JT65 the first three), and the band you pick is the band that goes on the wire. Nobody
is known to have been caught by this — what is on record is that Nexus could send it.

Centimetre bands are now converted by the same rule the three hand-written ones already followed,
so 13 cm goes out as "0.13". **The three values that have always gone out are unchanged**, no band
was added to any list, and nothing else about what Nexus broadcasts moved.

### Fixed: the ⊞ Panels menu no longer offers a checkbox that changes nothing, and it gains a Voice Keyer entry

From the bench: "what do the Panels selection / deselection of Rig Scope Controls and TX meters
do on the Phone tab? I don't see them anywhere on my screen whether enabled or disabled. Nothing
falls away when I uncheck either of them."

Both entries were live checkboxes with nothing behind them on that station. **Rig Scope Controls**
— and its twin, **Scope Controls** in CW — drive the radio's *own* panadapter: the span you set
there changes the hardware sweep, not the on-screen zoom. On the audio bandscope there is no rig
scope to command, so the pane cannot appear however the box is ticked. **TX Meters** work, but in
Phone and CW the meters read only while you are keyed, so on receive there is nothing on screen
either way — and the one moment the tick shows is mid-over, when you are not in a menu.

The menu now says which is which, the same way the DSP row has always only offered the functions
your rig reports. An entry with nothing on screen behind it right now carries the reason in a line
under it — "your radio is not streaming its own scope — these appear with an Icom CI-V or
FlexRadio panadapter" — so you learn why instead of hunting for a pane that was never going to
mount, and the line clears by itself the moment a native scope streams. TX Meters carry "readings
appear on transmit" in Phone, CW and Operate alike. Nothing is removed from the menu: an entry
with a reason on it tells you more than a missing one.

The same two questions turned out to have five more answers in the same menus, and they read the
same way now. In Phone and CW, **DSP Functions / DSP Toggles** and **RX DSP Levels** depend on
what your radio reports over CAT — on a rig that reports neither, those panes could never appear
and the boxes did nothing. In CW, **Sent Echo** holds what you have transmitted this session, so
at every start-up it is empty and its box moved nothing at all until your first over; it now says
so, and the line clears when you send.

The line explains the screen; the tick is still yours. Every box in the menu can be ticked and
unticked whenever you like, whatever your rig is doing, and what you choose applies the moment
that panel has something to show — untick **Sent Echo** at start-up and it stays away after your
first over, instead of making you transmit before you can hide a panel you do not want. Once you
have unticked one, its line goes: the panel is off your screen because you said so, and a note
still claiming your rig is the reason would be telling you something that is no longer true. Tick
it back and the line is there again in the same instant, if it still applies.

The menu also gains one entry it never had: **Voice Keyer** in Phone. If you work with a mic and
never touch the F-key slots, you can now put that pane away and the panes around it take the room.
It was held out until now by a blunt rule — nothing that transmits gets a checkbox — and the rule
is narrower now, because what it was really protecting is one thing: **you must never be unable to
stop a transmission.** That is held by the controls that sit outside the panes entirely: on Phone,
PTT in the transmit bar (and the Space bar, which is the same key), Stop TX and Tune in the header;
on CW, Stop TX and Tune, and Esc, which does what Stop TX does; on the Operating screen, Stop TX and
Tune in the operating strip, and Esc; on RTTY, Stop TX, the TX-enable button and both aborts in the
transmit bar; on SSTV, Stop and the TX-enable button. None of them is in a Panels menu, so no
combination of ticks and no saved layout can put them out of reach — that is the guarantee, and it
is the only thing the rule protects.

Two buttons on the Operating screen are worth naming because they are *not* on that list, and an
earlier draft of this note said they were. **TX On/Off** turns transmit off for the next cycle and
lets the over already going out finish — that is deliberate, it is how WSJT-X's Enable Tx behaves,
and the button says so when you hover it. **S&P** stops a CQ run and drops what is queued behind it;
it does not cut the over on the air. Stop TX is the immediate halt on that screen, and Esc reaches
it without the mouse. On RTTY and SSTV the TX-enable button *does* cut the over in flight, because
those modes have no next cycle to wait for — which is why it is on their lists and not on Operate's.

Everything else follows from it. Whether a pane can *start* a transmission has nothing to do with
whether you can put it away — the Operating screen's Tx messages and its decode lists have always
been yours to hide, and Stop TX sits in the operating strip either way. And a pane may carry a stop
button of its own, which goes away with the pane: the voice keyer's **■ Stop** is one, and RTTY's
**Auto on** toggle is another. Those are conveniences sitting on top of the guarantee, not the
thing holding it up. Put the voice keyer away mid-message and you lose its ■ Stop — and Stop TX,
Tune and PTT are all still exactly where they were.

Separately, and because it is worth telling you rather than because anything requires it: unticking
Voice Keyer while a message is playing stops the message rather than leaving it transmitting behind
a pane you just closed, and it throws away a recording you are part-way through making. Neither is
what "hide a pane" sounds like, so the entry says both before you tick.

**Undo last change** puts the keyer away by the same door, so it now says the same thing before
you press it. (Untick the keyer, tick it back, start a recording, press Undo — the recording used
to go with no word at all.) And whichever door you came through, Nexus says what it did when it
did it: an over it cut short, or a recording it binned — so walking off the Phone screen without
opening a menu tells you too. Stopping an over used to be the silent one of the two, which was
backwards. If the recorder cannot be stopped, that is what it says, instead of reporting a discard
that did not happen.

Reaching all of this from the keyboard works the way the rest of Nexus does: every entry is a tab
stop, takes focus with an undimmed focus ring, and reads its reason aloud with the panel name —
the operator who cannot see the line under it is the one it was written for. **Esc** closes the
menu and puts focus back on the ⊞ button you opened it with, instead of dropping you at the top of
the app to tab your way back down. A panel you have torn off still reads as "popped out", but the
words no longer run into the panel's own name when a screen reader speaks it — and that tag no
longer sticks. Only the Operating screen's panels were being un-popped at start-up, so a "popped
out" recorded anywhere else stayed on the entry for good, over a panel that was in fact sitting
docked in front of you. Every screen's panels are checked at start-up now. And the menu itself no
longer runs off a short screen: with eight entries and their reasons it scrolls inside its own
box, so **Undo last change** and **Reset layout** stay where you can reach them at any window size
or UI zoom.

### Fixed: the focus outline in light mode was a hair under the readable minimum

The ring Nexus draws around whatever has keyboard focus is one colour used everywhere. In light
mode it measured 2.999:1 against the page background, where the accessibility standard for a focus
indicator asks for 3:1 — not enough to look broken, and enough to be genuinely hard to follow on a
bright screen or a laptop panel at an angle. It is the same blue, one shade darker, and it clears
the bar on the page, on panels and inside the ⊞ Panels popover. Contrast for the ring is measured
in the test suite now rather than inferred, in both themes.

### Fixed: a rotator that stops answering no longer takes the dial with it

Reported from a live pass: "I saw a first Doppler shift, then it snapped back to the none
statement", and "after 5 seconds it snaps back to 'None — leave the dial to me', even though I
didn't change it."

One cause behind both. When rotor commands stopped getting answers, the track gave up on the
rotator — correctly, rather than hammering it for the rest of the pass — but it ended the whole
pass on the way out. That ran the end-of-pass handback: your transponder went back to none,
Doppler stopped, and the picker in the Satellites section reset itself a couple of seconds later.
A track that never had a rotator was fine; a track whose rotator quit lost the radio too.

The mast and the dial are separate things now, and losing one does not surrender the other. A
rotator that stops answering is let go and nothing else changes: the pass clock, the Doppler
correction and the transponder you picked all run to a real LOS. The track stops claiming an
antenna — it drops to Doppler only and shows no commanded angles, because it is not commanding
anything — and it tells you, once, that the rotator went quiet and the pointing is yours. The sky
dome keeps showing where the bird actually is, which is what you need to turn the antenna by hand.

Losing the pass itself is unchanged: the bird setting hands your dial back, and so does the rare
case of orbital elements the propagator can no longer follow — there, the same model computes both
the pointing and the Doppler shift, so there is no correction left to make and holding your radio
would be a lie.

The rotor readout in a cockpit header keeps up with it. That strip shows a dim "ROTOR —" when a
rotator is configured but silent, and it used to show only that — so a pass that had just lost its
rotator, and was still steering your radio, went nameless everywhere outside the Satellites
section. It now shows both: the honest dash for the mast, and beside it the bird, what Doppler is
holding, and the ■ that stops the track.

At the end of a pass whose rotator went quiet, Nexus still sends one stop — the controller may be
back by then, and a stop can only ever take motion away. It does **not** run "park" or "go to
ready" for that pass: you were told the pointing was yours, so the antenna stays where you left it.
A pass that kept its rotator parks as configured, unchanged.

### Fixed: changing band or radio no longer kills your PTT

Reported from the bench: work a satellite on the Icom, come back to Phone, pick 20 m — which
hands the station back to the Yaesu — and the PTT button pressed but the radio would not key.
Nothing on screen said why. The cure the operator found was leaving the section entirely: go to
FT8, come back to Phone, and PTT worked again.

Under the hood, a band change and a radio handoff both stop transmit — they have to, or the FT8
sequencer would keep calling a station that is no longer on your band, and a switch would leave a
carrier up on the rig you just left. Stopping transmit also drops the Enable-TX latch, which is
right for FT8: the sequencer would otherwise re-arm itself on the very next slot. But in Phone,
CW and RTTY that same latch *is* the microphone, and the Phone cockpit has no switch for it — so
the latch went down and stayed down, your press was quietly discarded, and only re-entering an
operating section (which arms transmit on the way in) put it back.

Those halts now put the latch back exactly where you left it, in Phone, CW and RTTY only. The
carrier is still cut, a held PTT is still released, and the newly selected radio still comes up
unkeyed — the switch just does not take your microphone away with it. Everything it never should
have touched is untouched: FT8 still stands down on a band change, TX Off still means off across
a QSY, a tripped transmit watchdog still holds, and Stop TX still means stop in every mode.

This covers every way the active radio can change — band routing, satellite routing, band
coverage, and the radio button in the top left — plus a band change with no switch at all, and
spinning the rig's own VFO across a band edge.

Keeping the microphone across a switch means the radio loop can now be asked to transmit during
the moment a two-radio handoff is still in flight — the fraction of a second where the app is
holding the radio you just switched *away* from. It will not: nothing keys in that window, and the
same rule covers a tune carrier, a queued CW word, an RTTY over, an APRS beacon, a voice message,
an SSTV image and an FT8 slot. Most of that work is held rather than dropped and goes out on the
right radio a moment later — a mic press you are still holding, a tune, a queued CW word. What is
timed to a slot is not held: an FT8 over whose slot boundary passes during the handoff is missed
rather than sent late, which is the right answer for a mode where a late transmission is a wrong
one. Testing CAT works the same way: while the port is handed to the baud-ladder probe, the app
will not report a key it cannot actually send.

One thing deliberately does not follow you: another program sharing the radio through the CAT
broker. Your own microphone stays live across a band change because you made it; a shared client
cannot see that you moved until its next poll, so its key is refused until you arm transmit
again — exactly as before.

### Fixed: picking 160, 80 or 40 m in Phone no longer lands on a locked-out frequency

Pick 40 m from the Phone band list as an Extra and the radio landed on 7.1250 — the exact bottom
of the Extra phone band. On the low bands you transmit below the dial, so a signal there runs
2.8 kHz *under* the band edge: the app locked transmit out, the PTT button came up 🔒 TX LOCKED,
and it did it on a band you are fully licensed for. The same thing happened on 160 and 80 m, at
every license class.

The band list now parks you a full sideband clear of the edge — 7.1278 for that Extra, 7.1778 for
a General — which is the first frequency you can legally key on that band. It is the same place
switching *into* Phone has always landed; the band list was the one path that had its own idea.
The high bands are unchanged: there you transmit above the dial, so the segment start was already
clear.

### Fixed: "pin this radio" on a pass now holds

Reported from the bench: "I tried pin this radio and it goes pinned, then goes unpinned." The
switch showed 🔒 for a moment and flipped itself back to 🔓 a couple of seconds later.

Peg-lock belongs to the live radio roster, alongside which rig is active and how bands route to
it. Saving the settings form can never move any of that — a form loaded before you switched rigs
would otherwise yank you back to the old one — so the roster is put back exactly as it was after
every save. The pin on the pass rail was written as a settings save, which meant it was one of
the things being put back: the click went nowhere, the switch lit anyway, and the rail's own two-
second re-read of the setting showed the truth and turned it off again. It now goes through the
control that owns the setting, the same one behind the 🔒 in the top bar, so it sticks until you
click it again.

Worth knowing if you saw this: the pin was not merely failing to display. It never engaged, so a
transponder pick could still hand the bird to whichever rig your routing preferred, exactly the
thing you clicked to prevent.

### Fixed: a Doppler correction no longer rewrites the mode every three seconds

From an operator's CI-V trace of a live pass: 110 seconds carried 38 mode commands and 38
data-mode commands — one pair per Doppler correction — on a bus already busy with the dial, the
meters and the panadapter. On an Icom a mode write can bump the filter with it.

A Doppler step was arming the same "apply this now" flag an operator's own QSY uses, and that
path re-asserts the mode deliberately: picking CW while already on a CW frequency has to command
the radio to CW. It also clears the give-up ladders — and that was the worse half. Re-armed every
three seconds they could never fire, so a radio that cannot reach a downlink would be asked for
the whole pass instead of being given up on once.

Corrections now move the dial and nothing else. They reach the radio on exactly the same loop
pass they always did — the mode still follows a bird change, a section change or a re-arm, and
the dial still gets there every few seconds.

### Added: Lock on — put the radio back on the bird

Move the dial inside the transponder's passband and Nexus already follows you: that is you
chasing a station across the passband, and your uplink moves with you (mirrored, on an inverting
bird). Move it *outside* — by hand, or because the rig came back somewhere else — and the dial is
somewhere the pass does not describe, with no way back short of hunting for the transponder you
already had selected. **Lock on** is that way back: one click re-runs the pick you already made,
so the routing, the band, the commanded mode and both legs all come with it, and you land back in
the middle of the passband.

It sits on the **Dial** line under the bird's name, with the line that names the rig being driven,
and it is there from the moment you pick a transponder. That is the point: picking one tunes the
radio straight away, so the dial is live — and can get away from you — long before you arm a pass
and long before the bird is up. The way back is there through all of it: pick made and nothing
armed, a pass armed and waiting for AOS, and mid-pass with Doppler correcting. The one state it
stays out of is the one where it would have to guess. With no transponder picked there is nothing
to put you back onto, and picking one for you would be picking your uplink.

### Fixed: the radio mode now follows the transponder, instead of staying in FM

Working a pass, picking an FM bird and then a linear one left the radio in FM — on a linear
transponder, which is silence. Reported from a live session: "should I expect the radio modes to
change when I move to each one like in other areas? It's not; it's staying in FM."

The satellite path could force FM and could force nothing else. An FM bird (SO-50, AO-91, the
packet digipeaters) parked the rig in FM, correctly — but a linear, CW or beacon transponder
asserted no mode at all, so the radio fell back to whatever the terrestrial section policy said.
For a station whose Phone mode is FM — one station-wide setting, written whenever you tune an FM
repeater, and reset by nothing — that policy answers FM on every frequency above 29 MHz. SO-50
and RS-44 are both on 70 cm, so nothing incidental cleared it either. Picking a hand-chosen FM in
the Phone cockpit during an FM pass had the same effect and lasted longer: that choice only
expired on a band change, so it outranked every later linear pick for the rest of the session.

Now a held transponder names the mode in both directions, and a new pick re-asserts it over a
mode chosen for the previous bird. What the transponder decides is FM-versus-linear and **which
sideband**; what your operating section decides is the form — plain SSB in Phone, the DATA
submode in Digital, CW in the CW section. So FT8 through a GEO transponder still gets DATA (plain
USB there transmits no RF on a normally-wired rig).

**And you can always overrule it.** The transponder's mode comes from SatNOGS, whose vocabulary is
open and crowd-maintained, and plenty of birds get worked in ways nobody wrote down — so picking a
mode by hand in the Phone cockpit during a pass now wins for the rest of that pass, on an FM bird
as well as a linear one. The uplink stands down at the same moment rather than swapping sidebands
underneath you, which is what it already did. Picking a transponder again is you asking for that
bird's tuning back, and re-asserts it. (Previously an FM bird could not be overruled at all: it
was decided above the mode picker, which is exactly the case Nexus is most likely to have wrong.)

Two judgement calls worth stating, because they are deliberate:

- **A transponder that advertises CW is worked in USB, not CW.** Through a linear transponder you
  copy the tone inside the SSB passband — that is how it is done, and it is what everything in
  Nexus's passband and Doppler model already assumes. Commanding the rig's CW mode would make the
  displayed dial mean something rig- and menu-dependent while Doppler is steering it, and it would
  lose an inverting transponder's sideband swap entirely. A CW **beacon** is not special-cased for
  the same reasons; it copies perfectly well as an audio tone in USB.
- **A transponder that publishes an LSB downlink is now put in LSB** and its uplink mirrored
  accordingly. That was an inverted-sideband error before — the exact mistake the transponder
  machinery exists to prevent — and on an inverting bird it inverted the uplink with it.

Data downlinks (BPSK, PSK31, FT8, MFSK and the rest) are worked on the linear path as they always
were; only the FM-carried ones (AFSK, FSK, GMSK, DUV, satellite SSTV) command FM, unchanged.

### Fixed: a satellite downlink Nexus cannot name is no longer refused

"There are some frequencies that say in sat that it isn't in my band plan — you should allow me
to go to those." Correct, and the refusal was ours, not the law's. Nexus's band table stops at
23 cm, so QO-100 at 10.489 GHz — and every IC-905-class microwave bird — was declined with
"outside the band plan", on an operator running Open class with no restriction of any kind.

The band table exists to route a QSY to the right radio and to label a contact. It is not a
permission system, and it was never asked whether the tune was legal. A downlink it cannot name
now tunes like any other, and the missing label is reported as **missing** rather than guessed:
the pass rail drops its band chip but still names the rig and the frequencies, the log strip
drops the band slot instead of printing an empty one, and the logged band is left empty with the
frequency carrying the truth.

Routing still works, and works from what you actually wrote. A rule that names bands cannot claim
a dial that has no band, and no per-radio band list is consulted (asking either with a blank would
have matched every catch-all and sent the bird to whichever rig owns "everything else"). But a
rule with no band selector — "satellite work goes to the IC-9700", the shape Nexus itself
recommends — answers the same for every band and therefore for no band, so it still routes these
birds to the rig you designated. Where nothing answers, the existing fallback applies: if the
active rig cannot reach the downlink and exactly one other radio could, the pass goes to it.

Nothing about transmit permission moved. The licence-class gate is a separate check on the
transmit path and is untouched: an Open-class operator may key 10 GHz, and a Technician or
General is still locked out there exactly as before. The 0.27.0 refusal that declines an
unverifiable cross-band split on the Main/Sub Icoms is also unchanged — it deliberately fails
closed when it cannot prove both legs share one band, which is still the case above the table's
ceiling, so the band table was fixed around rather than extended.

### Fixed: a refused satellite uplink now says what to change

Both satellite split refusals were true and neither told the operator what to do about it. On
an IC-9700 holding a cross-band bird under a VFO A/B mapping, the CAT status explained that
Nexus has no verified cross-band A/B split for that radio and that nothing was written — and
stopped there, with the working layout one selector away and unnamed.

Each refusal now ends with a cure clause chosen by the radio in play, using the same rule the
CI-V daemon itself uses to decide whether it can serve a rig:

- **An IC-9700 or IC-905 on a serial/USB port.** The A/B refusal names the layout that does
  carry a cross-band uplink there — Main = downlink / Sub = uplink — and both places it can
  be picked (Settings ▸ Radio ▸ Satellite Doppler, or the mapping selector on the pass rail).
  Ask for that layout while something other than Nexus's own CI-V backend is serving the
  radio and the refusal names the switch that changes it (Settings ▸ Radio ▸ Rig Control ▸
  Native Icom CI-V), plus Test CAT for what is serving right now. It names the switch rather
  than telling you to turn it on, because it may already be on.
- **An IC-910 or IC-9100, and any radio reached over the network rather than a serial port.**
  There is no cure to name, so none is invented: the refusal says the native CI-V backend has
  no path to that radio, so no VFO mapping carries that pass's uplink there. No switch to
  hunt for, and no selector to work through one entry at a time.
- **Every other radio that can reach the Main/Sub refusal** — the mapping can be picked on
  any rig — gets the same honest dead end.

What is refused has not changed, and neither has the transmit path: these refusals still
write nothing to the radio, still transmit nothing, and still leave the receive dial being
corrected wherever it already was.

### Added: the pass rail offers the mapping that can carry the pass

Naming the working layout in the CAT status is one thing; you still had to go and set it. The
pass rail now offers it — one click, on the button that is already there.

Nexus suppresses every uplink offer once you have chosen and confirmed a mapping, so it can
never nag you into replacing your own choice. That rule is right everywhere except one place:
when the mapping you chose **provably cannot carry the pass you are on**. Then the rail shows
**switch mapping**, naming the layout that can and the radio it applies to, and the Doppler
row stops reporting an uplink it is computing and having refused every tick.

Offering a correction to a choice that cannot work is not overwriting your choice. Nothing
changes until you click:

- The offer appears **only** when the uplink was genuinely written and genuinely refused for
  this pass, on a radio where Nexus's own CI-V backend is switched on and can serve it — i.e.
  only when switching actually gets you an uplink.
- A mapping that works is **never** second-guessed. Work a V/V bird on VFO A/B on the same
  IC-9700 and the rail says nothing: that is how that pass is worked, and the split lands.
- Where no working mapping is known — an IC-910, an IC-9100, a network-connected rig, or an
  Icom whose native CI-V is switched off — nothing is offered, because there is nothing that
  one click could fix. Those stations get the refusal text above instead.

The click goes through the same confirmation path as every other mapping change, records it
for the radio the button names, and still means Nexus writes nothing to the transmit VFO you
have not consented to.

## [0.27.0] — 2026-08-02

### Fixed: the log strip no longer invites an accidental log entry

On the Phone and CW log panel, the callbook button sat in the same row as Log — and that row
wraps to fit whatever width the log column has, so Log did not stay in one place. Reaching for
a lookup mid-QSO could commit the contact instead, and several contacts got logged that way.

Log and Spot now have a row of their own, directly above the caller card and immediately under
the line that spells out what will be written ("logs to the shared logbook as SSB · 20m ·
14.200 MHz"). Read it, then commit — and Spot stays right beside Log, where it has always been.
Nothing else about logging changed: the same fields, the same Enter behaviour (Enter on a fresh
call still pulls the callbook first, then logs), the same Clear button beside the callsign.

The callbook button is also now labelled **Lookup** rather than QRZ, and is styled as the quiet
utility it is instead of a second action button. Lookup is the honest name: it asks QRZ first
and falls through to HamQTH, so it has never been QRZ-only, and it works with a free HamQTH
account and no QRZ subscription at all.
### Fixed: a satellite uplink Nexus cannot verify is now refused instead of reported applied

Hold a linear bird on an IC-9700, pick "Main = downlink, Sub = uplink" — the layout the
radio is actually built for — and the pass rail answered "the uplink was not written". It
never said why, and the mapping the operator picked was the right one for their rig.

The missing piece was which CAT backend was serving. Nexus can drive an IC-9700's satellite
mode two ways, and only one of them is wired: **Native CI-V**, where Nexus owns the CI-V port
itself, engages the rig's satellite mode, writes the uplink into the Sub band and reads every
step back off the radio before reporting it done. Native CI-V is off by default, so the usual
IC-9700 station runs on Hamlib's `rigctld` instead — and this build has no verified satellite
split there.

- **"Main = downlink, Sub = uplink" when Native CI-V is not what is serving.** Nexus now
  sends nothing at all — no split command, no transmit frequency — and the CAT status line
  says so in one sentence: Nexus drives that layout only through its own native CI-V
  backend, that backend is not what is serving this radio, nothing was written and nothing
  was transmitted, and your receive dial is still being corrected. It does not guess which
  backend *is* serving. Nexus may have started `rigctld` itself, fallen back to it when its
  own daemon failed to start, or simply attached to a `rigctld` someone else launched — and
  in that last case it has never even read the radio's model. Test CAT names the one in use.
  Before, the split went out, and a `rigctld` that answered "ok" was enough for the pass rail
  to display a transmit frequency the radio was not on.
- **Every uplink mapping that rides VFO A/B on the IC-9700, IC-910, IC-9100 and IC-905 — a
  behaviour change.** Nexus has no verified cross-band VFO A/B satellite split for these four
  radios; sending one unverified is how 0.24.2's "uplink" went nowhere. The rule is now that
  the uplink is written only where Nexus can place **both** legs of the pass on the same
  band. A cross-band pass is refused — and so is a pass it cannot place on the band plan at
  all, because then it cannot prove the two legs share a band. That second case is not
  hypothetical: the band plan stops at 23 cm, so an IC-905 microwave bird is refused even
  when both legs really are on 10 GHz. "Transmit only" is refused on these four radios for
  the same reason — that mapping never writes a receive dial, so there is no second leg to
  compare against. **V/V and U/U birds up to 23 cm keep the A/B split**: both legs on one
  band is the case A/B carries, and it still carries it. Every other radio — the FT-847,
  TS-2000, TS-790 and FT-736R class, and every ordinary HF rig — is untouched and still
  takes a cross-band uplink on VFO B as before.
- **Settings no longer pre-fills a mapping it cannot drive.** The one-click "Main = downlink,
  Sub = uplink" offer now appears only where Nexus's own CI-V engine can actually serve the
  radio — an IC-9700 or IC-905 on a serial/USB port. The IC-910 and IC-9100 are outside that
  engine's coverage in this build, and so is any radio reached over the network rather than a
  serial port, so those stations are asked instead of pre-filled. "Serial" here means exactly
  what the CI-V daemon itself means by it, so a settings file written before Nexus had a
  connection setting counts as serial, the same as it does everywhere else in the app.

Doppler keeps correcting your **receive** dial wherever it was already correcting one, so no
refusal above costs you the downlink; work the uplink from the radio's own front panel for
that pass. On the A/B side a refusal does cost something real, and it is worth being plain
about: the IC-910, IC-9100 and IC-905 used to get an uplink attempt on a cross-band pass, and
now they do not. That attempt was only ever *acknowledged* — the radio answered "ok" and
nothing in this build read the frequency back off it — so what it bought was a number on the
pass rail that may or may not have been where the radio was transmitting. Losing it is the
safer half of the trade: on a linear bird, a split you believe and the radio did not take
puts your carrier in the transponder's downlink passband, on top of everyone else working
it. (On the IC-9700 that attempt is known not to land — it is the 0.24.2 bug.) Ordinary A/B
splits — including every terrestrial pile-up "UP 5" — are untouched on every radio and on
both backends.

The Hamlib satellite recipe stays deliberately unwired rather than sent hopefully. Hamlib can
answer a frequency read-back out of its own cache, so a read-back a stale value can satisfy
cannot tell a landed uplink from an echo of what we just asked for, and confirming the
difference needs a real radio on a real `rigctld` — which neither this tree nor CI has.
Guessing wrong there puts your carrier in the transponder's downlink passband, on top of
everyone else working the bird.
### Fixed: switching modes no longer forgets your frequency

Working a station on 20 m phone at 14.240, a stray click into CW and back reset the dial to
14.225 — the start of the phone segment — and the contact was gone. Nexus now remembers the
frequency you were using, per band and per mode, for the whole session. Switch from Phone to
CW and back and you are on your own frequency again, not the segment default.

Every band keeps its own memory. Leave 20 m phone for 40 m CW and your 14.240 waits: switch
to Phone on 40 m and you get your last 40 m phone frequency (or the usual segment start if
you have not used one yet), and picking 20 m from the band dropdown brings 14.240 back. Every
mode remembers on a mode switch — Phone, CW, RTTY, and the digital Operate section — and the
band dropdowns in the Phone and CW cockpits restore the band's memory too. Anything that
names an exact frequency still goes exactly there, with no memory overlay — a typed MHz, a
band-plan channel, a spot or Needed click carrying the spot's own frequency.

The first visit to any band/mode this session behaves as it always has, and a restart starts
genuinely fresh: the memory is never saved to disk, and the frequency Nexus starts on is not
counted as one you chose — neither the dial in the settings file nor the one read off the rig
when the CAT link first opens. It cannot be: those say where the radio IS, never who put it
there, and that could as easily have been APRS, a repeater, an ISS pass or a satellite as you.
So closing Nexus parked on the national APRS channel and opening it the next morning will
never turn 144.390 into "your 2 m phone frequency". Memory starts the moment you tune
somewhere yourself. License privileges are still checked on every return — if your class
changed mid-session, the dial falls back to the legal segment start.

Turning the VFO knob counts as choosing a frequency, with two exceptions. The first dial the
CAT link reports is not a knob move at all — it only says where the rig already was, as above.
And while Nexus is holding the dial on one of its own channels — APRS, an FM repeater or
simplex channel, the ISS SSTV downlink, a satellite transponder — tuning around inside it is
working that channel, not picking a frequency for the band, so it is not remembered. Move the
knob to another band and the dial is yours again. The knob reports a frequency and nothing
else, so a frequency you reach that way comes back on the band's usual sideband rather than
the one you happened to be using on the band you left.

Two coarsenesses worth naming. An FM channel or FM-bird hold stays in force until you tune
somewhere explicitly (that is what keeps the rig in FM instead of dropping it the moment you
nudge the VFO), and while it is set any knob move at 29 MHz or above counts as the
machinery's, whichever VHF/UHF band you are on; APRS is held to 2 m only. The channel mark
Nexus sets when it tunes one of its own channels lasts the same way: it is dropped at the next
frequency you choose yourself — a band pick, a typed MHz, a mode home, or a knob move onto
another band — and until then a knob move inside that band is the channel's. It names the one
band the app parked on, so anywhere else it says nothing at all.

Frequencies the app tunes for its own machinery are never mistaken for yours, and never the
other way round either. Each part of Nexus that moves the dial says whose frequency it is at
the moment it moves it, so the answer never depends on when the question is asked:

- Work a satellite from 20 m phone and your 14.240 is still waiting when you come back.
- A pass that ends leaves the radio parked on the bird. That Doppler-corrected frequency is
  never adopted as your frequency on 2 m, however long you stay there afterwards — and
  neither is a spot you hand-tuned to inside a linear bird's passband.
- Tuning APRS from 146.520 keeps 146.520 as your 2 m phone frequency, even though 146.520 and
  144.390 are the same band and nothing about the move looks like leaving it.
- Switching radios banks the dial you were on before the handoff. The memory is station-wide:
  a recalled frequency routes to the radio that owns that band and mode, exactly as any other
  band change does.
- The ISS SSTV auto-arm's 145.800 is one of those channels too: your 2 m frequency banks on
  the way in, the 145.800 itself is never remembered, and hand-chasing the pass's Doppler
  inside the armed window does not overwrite what you were on either.

Two smaller things. Picking the band you are already on from the dropdown now keeps the
frequency you are on, instead of snapping back to the segment start. And that dropdown's
tooltip says what the control does: your last frequency on that band in that mode, else the
start of your licensed segment.
### Added: hear the pass — AOS and LOS alerts for the armed track

Arm a pass with "Work this pass" and walk away from the screen: the moment the bird rises,
Nexus now plays a rising three-tone and shows a loud popup with the facts you need at the
rotor — the bird, the azimuth it rises on, how high the pass peaks, and how long you have.
At LOS a falling tone (tellable from the rising one blind) accompanies the pass-complete
notice, which keeps saying exactly what was handed back — the dial, the uplink split, and
whether the rotor is about to park or move to ready on its own.

The alerts fire wherever you are in the app, not only with the Satellites section open —
the pass-complete notice used to be the section's alone, so working the Logbook at LOS
meant missing it. They report the track's own state transitions, never a wall-clock timer:
if the machine slept through the rise, waking mid-pass says "pass in progress" with the
minutes left instead of a stale "starting now", and waking long after LOS says nothing.
Alerts never touch the radio, the rotor, or the track — they only tell you what happened.

This is the armed track's own channel, separate from the per-pass ⏰ alarm on the schedule.
An alarm lead ("wake me 15 minutes before") still fires on its own moment; at AOS the two
channels coordinate so you never get two popups for the same rise.

The popup never steals keyboard focus and never blocks the screen. The tones are on by
default — that was the ask — with a "Pass alert sounds" switch beside the satellite
Doppler settings for operating from a quiet shack; the popups stay either way.
### Changed: the pass strip says what it ranks — "Next" and "Best 24 h", over every bird

The strip above the Satellites schedule was labelled "Next up" but ranked by pass quality,
so a spectacular pass hours away could sit above a workable one rising in ten minutes —
entries "further away than what's in the schedule", as the field report put it. It also
read only your ★ favorites, so a good pass on an unstarred bird could never surface there.

The strip is now two labelled pairs, drawn from every workable bird over your grid: "Next"
is the two soonest passes in clock order — a pass already in progress leads, marked
"already up" when it rose before the scan window — and "Best 24 h" is the two
highest-quality passes in the next 24 hours, by the same elevation-first rank the old
strip used. A pass that earns both spots shows once, under Next, and Best fills in with
the next one down. ★ marks your favorites, the mode pill ([FM voice], [Linear SSB/CW], …)
rides every classified row, and ▶ Work runs the same chain for any bird, starred or not.
Needed-grid chips stay on ★ rows — they come from the favorites schedule, which is the
only place that computes them.

The strip admits exactly the birds the "Other birds overhead" band admits — one rule, so
a bird that band refuses (reported dead or re-entered, placeholder rideshare names) never
appears in the strip either. The schedule below and the band are unchanged.
### Added: hide the countries you have stopped chasing

On a busy evening most of a waterfall is a handful of countries you worked years ago. Band
Activity gains a **Countries** button beside the filter chips: tick the ones you do not want
to see and they leave Band Activity and the Call Roster.

The list is 18 — the high-density entities an experienced chaser actually wants gone, rather
than the full DXCC table with a search box:

United States (K/W/N) · Canada (VE) · Mexico (XE) · Germany (DL) · Italy (I) · Spain (EA) ·
England (G) · France (F) · Japan (JA) · Brazil (PY) · Argentina (LU) · Poland (SP) ·
European Russia (UA) · Ukraine (UR) · Netherlands (PA) · Czech Republic (OK) ·
Slovenia (S5) · China (BY)

It is a view filter and only a view filter. Decoding, logging, the QSO sequencer and your
needed-entity alerts never see it, so nothing you have switched off can cost you a contact:

- a station **calling you** always shows, whatever its country;
- the station you are **working** stays put mid-exchange;
- **needed outranks excluded** — a new entity or a new band slot from a country you have
  hidden still comes through.

Matching is on the DXCC entity the callsign resolves to, not the letters it starts with, so
`VE3XYZ/W1` counts as United States and `W1ABC/VE3` as Canada, and hiding Germany catches
DA, DB, DJ and DK as well as DL.

While anything is hidden both panes carry a **"3 countries hidden"** chip with a Clear
beside it, so a thinned band is never a mystery — and the "N heard" and roster counts state
what is on screen, not what arrived. The Rx Frequency pane is deliberately left alone: it
answers "what is on my frequency", and a station you are not chasing sitting on top of you
is exactly what you need to see.

Your choice is remembered per station, not per window — pop out a band map and it shows the
same band as the main window.
### Added: full FT8/FT4 decoder configuration — AP on/off, CQ-only AP, single decode

The Decoder section (Settings ▸ Weak-signal digital) grows from depth-and-passband to the
full set of receive controls the built-in WSJT-X decoder actually takes. A-priori (AP)
decoding — the hypothesis-assisted passes that pull marginal replies out of the noise using
your call, the DX call and the QSO state, including the cross-cycle replay — now has its
WSJT-X "Enable AP" switch, on by default exactly as before. A second control restricts AP to
the bare CQ hypothesis, the guard WSJT-X applies by itself after five idle minutes; Nexus
makes it an explicit choice. And Single decode narrows the search to ±25 Hz around your RX
marker — the same one-station window WSJT-X uses for a double-click re-decode — to isolate
one weak station instead of working the whole passband. Single decode is FT8 and FT4 only:
50 Hz is narrower than a single JT65, Q65 or MSK144 signal, so those modes keep the full
passband whatever the switch says.

Every control is wired through to the decoder itself and proven there: tests assert at the
decode-job boundary that flipping a switch changes what the Fortran is called with, and
behavioural tests show AP-off really does silence the FT8 AP passes and the cross-cycle
replay, and that CQ-only really does cost FT4 the deep-hypothesis recoveries.
Nothing placebo. Honesty notes, stated in the UI too: the AP on/off switch is FT8-only —
FT4's decoder has no such flag (its AP is part of Normal/Deep depth), though the CQ-only
restriction applies to both; and on a WSJT-X UDP companion source, decodes arrive already
made, so none of the Decoder settings act. Defaults are untouched — leave the section alone
and the decoder behaves byte-for-byte as it did.

### Fixed: the caller card's distance and bearing now agree with QRZ

Work a station on Phone or CW and the card that appears when the call resolves reported a
bearing that did not match the one on the station's QRZ page. The maths was right — the
inputs were coarse. QRZ measures between the two stations' exact coordinates; Nexus was
measuring between the centres of grid squares, and a square is a box, not a point.

QRZ and HamQTH both hand us the station's real position in the same lookup that fills in
the name and QTH, and Nexus was throwing it away. It now keeps it, and the caller card
measures from it. Worked against a 4-character locator that is worth about a degree of
bearing and a dozen miles — W1AW from EN52 read 823 mi · 89°, and now reads 835 mi · 88°,
measured to the same pin QRZ computes its own figures from.

Nexus only accepts a position the callbook actually vouches for. QRZ returns coordinates
for every record, but tags where they came from — a real pin, a geocoded address, or a
fallback back-derived from the grid square or the DXCC entity. The last of those can sit
hundreds of miles from the station, so Nexus uses only the first two and falls back to the
locator otherwise. A station with no position on file is unchanged.

The larger half of the gap is your own grid. Everything is measured from where you say you
are, and a 4-character locator says only "somewhere in this ~100-mile square" — so Nexus
measures from the middle of it. On a DX path that costs about a degree, but on a station a
couple of hours away it is worth up to ~30°, whatever the other end does, and it puts every
distance out by up to ~60 miles at any range. The setup wizard used to tell you 4 characters
was plenty. It no longer does, Settings says the same, and the caller card's tooltip now
names whichever side is still a square instead of presenting the number as exact. Setting a
6-character grid closes it.

Distance is unchanged in method (great-circle, statute miles) and gains the same precision.
The Operate roster's bearing column and the map are untouched. So is rotator pointing, which
answers a deliberately different question — "point at this callsign's DXCC entity" — and
still swings to the entity's nominal centre, not to the station. On DX that is within a
beamwidth; for a stateside contact the two numbers are not meant to agree.
### Changed: the displays answer the audio — waterfalls, scopes and meters respond faster

The voice/CW/RTTY/SSTV displays lagged the ear. You heard a signal and the screen answered
late and rounded-off: the waterfall took a third of a second to bring a new signal to full
brightness, the RTTY and SSTV waterfalls threw away five of every six spectrum rows, and
both meters answered roughly half a second after the audio. Nothing here adds animation or
invented motion — every change removes real delay between the antenna and the pixels.

- The spectrum's analysis window is half as long. A key-down or a voice syllable reaches
  full brightness in ~170 ms instead of ~340 ms, and short CW elements stop blurring
  together. Nothing visible is lost: the finer raw resolution the long window bought was
  below what the display can draw, and computing the shorter window costs less CPU.
- The RTTY cockpit and the SSTV band waterfall scroll at 20 rows per second — the same
  cadence the Phone/CW rig scope already runs — instead of 8. The FT8/FT4 waterfall is
  unchanged: those modes are slot-synchronous and their display is right as it is.
- The RX audio meter has real instrument ballistics: it snaps up with the audio (90% of a
  step within ~60 ms) and falls smoothly, like a hardware S-meter. It used to smooth the
  attack and the decay equally, so the needle registered a signal ~150–300 ms late.
- Both meters read through a new fast lane, polled every 100 ms, that no longer rides the
  radio loop or the 300 ms status poll — so a slow CAT read can no longer freeze the
  needles for seconds at a time, and a meter whose readings stop arriving falls back to
  rest within about two-thirds of a second instead of holding a stale needle. On a
  healthy link the CAT S-meter is also re-read every 360 ms instead of every 750 ms —
  twice as fresh, without crowding the link the radio dial shares. Slow serial links
  keep the gentler cadence; that limit is the radio's, not the app's.
- The meter bars no longer ease their width in CSS. The eased bar smeared every real
  reading by another 80–120 ms; color changes still ease, measurements do not.
### Fixed: "TLE 26 days — STALE" on a catalog that is hours old

The Satellites screen carried an amber "TLE 26 days — STALE · refresh" chip, a warning
chip in the status bar on every screen, and a Settings line reading "The element mirror
is unreachable and your elements are 26 d old — import a fresh element file or retry
later." None of it was true, and there was nothing to do about it.

Every one of those surfaces was reading the age of the single oldest satellite in the
catalog and presenting it as the age of your elements. The catalog is 367 birds, and the
typical one is a few hours old — but a handful are legitimately old. AO-7 launched in
1974 and gets fresh elements when someone observes it; a few dozen more are re-observed
every few weeks rather than every day. One of those set the number for all of them.

Worse, the number could not come down. A satellite whose elements pass 30 days is
dropped from the set, so the oldest one still counted is always just under 30 — as one
aged out, the next in line inherited the badge. The chip was going to read stale forever
no matter how fresh your elements were, and the calm "your elements are current" message
the app already had could never appear.

The chip, the status-bar chip, the Settings line and the Connect Passes badge now report
the median age of the elements they draw on: what the typical satellite in your catalog
carries, not what the slowest-observed one does. On the catalog shipped with this
release that reads a fraction of a day. A set that genuinely goes stale still says so —
once more than half the birds pass the 14-day line the median goes with them.

Satellites held back by the 30-day ceiling are counted and reported in their own right,
so "my catalog is current and a few birds sit out" no longer reads identically to "my
whole set has gone stale". The Satellites header states the number beside the age ("367
birds · 30 sit out past 30 d"); Settings ▸ Radio ▸ Orbital elements carries it on the
line that is always there, not only while a refresh has failed; and every refresh result
— landed, blocked or failed — accounts for those birds.

Satellites in the 14-to-30-day band are counted too: still used, still drifting. A
median can hide that band, and this is the shape it hides — half your birds sitting at
29 days while the other half arrived this morning reads as a current catalog, because
the typical bird is one. The Satellites header carries both counts whenever they are not
zero, and when most of the elements you hold are past the 14-day line the status bar and
the Connect Passes pane say so as well. A slow-cadence tail on an otherwise current
catalog stays quiet, which is what the shipped catalog looks like.

The Satellites section always has a way to refresh elements. The amber chip was also the
section's refresh button, and it correctly disappears now that the reading is honest, so
a quiet "⟳ refresh elements" chip sits in the header whenever the amber one does not.

Unchanged: the per-satellite rules. A satellite past 30 days is still refused by name and
age when you arm it, arming a satellite with elements past 14 days still asks first, the
Birds list still explains every excluded bird individually, and a fresh bird still arms
cleanly no matter how old anything else in the catalog is.
### Changed: the sky dome carries azimuth and elevation on the satellite

Working a pass with a manual rotator means reading the azimuth and elevation off the
screen and turning the mast to match. Both numbers lived in a tooltip, so following the
pass meant holding the mouse on a moving dot to see where to point next.

Azimuth and elevation now ride on the satellite and stay there for the whole pass. They
sit on a plate that holds them legible over the pass track, and they move to the other
side of the satellite near the edge of the dome or when the antenna marker is on that
side — so they never run off the dome, and never cover the gap between where the antenna
is pointed and where the satellite is.

The satellite is drawn as a spacecraft — body and solar panels — rather than a plain dot,
and matches the mark the world map already uses for the same bird. The text readout under
the dome and the hover tooltip are unchanged.

### Changed: the rise and set marks on the sky dome carry their bearings

The two triangles on the rim of the dome are where a pass starts and ends, and they are
what you pre-point a manual rotator by. Their bearings lived in a tooltip. The set mark is
drawn as an outline rather than a solid triangle, and an outline answers the mouse only on
the line itself — so on that one, catching the tooltip meant landing the pointer on a
hairline.

Both marks now print their bearing on the dome: the rise or set bearing with its compass
point, on a plate beside the triangle it belongs to. Each plate names which mark it is, in
words and with the same up and down arrows the readout under the dome uses, so a pass that
rises and sets close together never leaves two bare numbers you have to tell apart.

The plates are azimuth only. Both marks sit on the horizon, so an elevation there would be
restating the picture rather than reporting anything.

A plate sits inside the dome next to its triangle and steps aside when that spot is taken —
it never covers either triangle, the satellite, the satellite's own readout, or the antenna
marker, and it stays inside the horizon at every bearing. The whole body of the set mark is
now hoverable, so its tooltip, which also carries the time, is reachable without threading
the mouse onto a line.
### Added: the satellite surfaces say how high the bird is

The Birds list, the world map's satellite hover and the sky dome each show the bird's
altitude — how far above the earth it is, right now.

It is the number that tells you what kind of pass you are about to work before you open
anything: a 630 km LEO screams over in ten minutes with the Doppler running away from
you, while an elliptical bird near apogee loiters for the better part of an hour with a
shift that barely moves. Down the Birds list the figures line up in a column, so a low
circular bird and a high elliptical one tell themselves apart at a glance.

Altitude is always labelled, and on the sky dome it sits directly under the range it is
easily confused with — range is how far the bird is from you, altitude is how far it is
from the ground. The dome's number is recomputed with the rest of the pass geometry every
three seconds, because on an elliptical orbit it genuinely moves.

A bird nothing carries current elements for shows no altitude rather than a zero, and
neither does an armed pass before the bird rises: there is no position computed yet, and
0 km would put a satellite on the ground.
### Fixed: the starter favorites now cover the birds you can actually work

The one-time favorites seed ranked every bird on pass count, and pass count is mostly a
statement about altitude. FO-29 flies an 800 × 1320 km ellipse at 13.5 orbits a day; a
480 km cubesat flies 15.3, so the cubesat shows more passes over your grid every day of
the year. Run the app's own pass predictor over the catalog it ships with — 24-hour
windows from six grids at four different start times — and the old ten came out 3 to 9
beacon-only telemetry cubesats out of ten, 6.5 on average: birds you can hear and cannot
work. That is the shape of the catalog rather than bad luck. 305 of the 367 birds that
carry elements have a downlink and no uplink, and 18 of the top 20 by orbital rate are
among them, so a pass-count ranking fills its ten mostly from birds nothing can be
worked on.

What happened to FO-29 is the other half of it. Across those same 24 runs it ranked
anywhere from 2nd to 81st, and it reached the top ten in 6 of them; some SSB/CW
transponder reached the ten in 14 of 24. Ranked on passes alone, whether you got a
workable bird at all came down to the geometry of the day you first opened Satellites.

The catalog now records what each bird is worked WITH — an FM voice repeater, a linear
(SSB/CW) transponder, a digital/packet channel, or a beacon downlink and nothing else —
and the seed fills its ten by rotating through those three workable kinds, each one
offering its own best bird under the same pass-count-and-elevation rank as before. A
beacon-only bird takes a slot only after the three are exhausted. On today's catalog that
is four linear, three FM and three digital — at every one of those 24 grid-and-window
measurements, with no beacon-only bird in any of them.

Nothing else about the seed changed. It still runs once ever, still never seeds over
anyone who has stars or ever had them, still requires a bird to be alive, to carry a live
amateur transmitter, to hold current elements and to fly a workable pass over your grid,
and still waits rather than spending itself when your grid or the catalog is not there
yet. A bird can hold more than one kind at once, which 60 of today's 372 active amateur
birds do — QO-100 carries a linear transponder, digital segments and a beacon — and each
one counts wherever it belongs.

Existing stars are untouched. This affects a first run only.

### Fixed: satellite SSTV puts the radio in FM

Clicking an SSTV row on the ISS — 145.800 "Mode V Imaging", 437.800 or 437.550 "Mode U -
SSTV" — set the radio to USB. Every SSTV downlink in orbit is narrowband FM, so the
picture arrived garbled. Satellite SSTV rows now set FM. HF SSTV is unaffected: it really
is an SSB mode, and no satellite transmits SSTV below 30 MHz.

### Known: a satellite that publishes an LSB downlink is still tuned USB

FO-82 (NORAD 40320) declares its transponder downlink as LSB. Picking it sets the radio to
USB and the uplink is then derived from that, so both ends come out on the wrong sideband.
It is one bird, and the fix belongs with the wider transponder-tuning work rather than
here.
### Added: other birds, without leaving your schedule

The Satellites schedule shows your ★ favorites, and the Birds list names every bird — but
nothing connected the two. Finding a workable pass on a bird you had not starred meant a
trip to an alphabetical name list that carries no pass information at all.

The schedule now carries a second, collapsed band under your favorites: "Other birds
overhead · N workable · 24 h". The count is live. Expand it and you get one row per bird —
its best pass in the next 24 hours, ranked by how workable it is — with the same star and
▶ Work controls your own rows have. Star a row and the bird moves up into your schedule,
gaining the needed-grid chips, the pass alarm and the 48-hour view. Collapsed, the band
adds nothing to the screen: your schedule is exactly what it was. Twelve rows show by
default, with "show all N" for the rest; placeholder rideshare objects that share a name
with their siblings are left to the Birds list, where starring one cannot star six.

With no favorites starred at all, the section no longer replaces the whole planning column
with one sentence — the schedule renders with the band open, so the answer is on screen
from the first visit.

Birds' mode class shows as a pill beside the name ([FM voice], [Linear SSB/CW], [Digital],
[Beacon]) on schedule and discovery rows, wherever the catalog can say — no class, no
pill, never a guess.

### Added: a way back out of a bird's detail

Opening a bird pinned its detail to the side column for the rest of the session — no
close, no Escape, and the Birds list starts more than a full screen below it. The detail
heading is now sticky with a ✕, Escape closes it too, and the tracking badge in the header
opens the tracked bird again with one click. Closing a detail never touches the track, the
rotor or the dial.

### Fixed: a pass already in progress reported a wrong rise time

The Satellites view's 24-hour pass scan started at "now", so a bird already above the
horizon was reported as rising at the moment you looked, with its peak elevation
understated to what remained — and the cached scan repeated that answer for up to ten
minutes. The scan now looks back six hours, the same way the schedule always has: a pass
in progress keeps its real rise time and its full peak. This also corrects the map view's
pass pane and the first-run favorites seeding, which read the same scan.
### Changed: the FT8 Classic view puts the decode window where you operate

Field feedback from an advanced DX operator, running Nexus beside WSJT-X on an
ultrawide: the Rx Frequency pane — the stream you actually run a QSO from — was a
small box at the bottom of the side rail, and too much of the screen went to chrome.

The Classic layout is now three columns: Band Activity on the left, a full-height
Rx Frequency pane beside it with the Tx1–Tx6 message machine docked underneath —
the same bottom-right geometry as WSJT-X — and the Stations roster on the right.
Your own transmissions appear in the Rx Frequency stream in yellow, interleaved
with the answers, so you click a call and watch the whole exchange line by line.
Clicking a decode there does exactly what it does in Band Activity. A drag handle
between the pane column and the roster lets you tune the balance; the split is
remembered.

The chrome got out of the way. The status line, the QSO panel's three rows and the
permanent TX-meters line are now ONE compact strip under the waterfall. Call CQ /
S&P and TX On / Tune / Stop TX / Hold Tx come first, anchored at the strip's left
edge, and they hold the same position whether the strip is idle, mid-QSO or
transmitting — the DX call appearing or the rig keying never moves Stop TX. After
the buttons: the TX state and sequencer readout with the DX call and report, what
is being sent now, the free text box (sized to the 13 characters it can actually
hold), and — moved into the strip from the old status line — TX AUTO / Skip Tx1
and the next-slot countdown, plus the SWR/ALC/Po/COMP meters in a fixed-width cell
that shows live bars while transmitting and the dimmed last readings between
overs, so nothing jumps when transmit starts. Every control that was on screen
before is still on screen, always visible, just smaller and closer together. The
header packs into one row on wide screens: the DXpedition mode is a dropdown,
Spot and Pop out are icon buttons, Record QSO is a dot, and the memory chips keep
to a single line.

At ultrawide half-screen the strip is a single row and the Rx Frequency pane
grows from roughly 15 visible decode lines to over 60; Band Activity and the
roster both gain height. On a 1366x768 laptop the three columns hold and the
strip wraps at a fixed point into two stable rows — buttons and readout above,
sending/free-text/meters below; on narrow windows everything stacks in one
column as before.
### Changed: Doppler corrects your downlink without being asked

Arming a pass and picking a transponder is the whole ask. Nexus corrects the receive dial
for the pass from that moment, with nothing to switch on first.

Before this, two settings stood between an armed pass and a moving dial — Satellite
Doppler, and the VFO mapping — and both shipped off. A station that had never opened
Settings ▸ Radio armed a pass, held a transponder, and watched the dial sit still with
nothing on screen saying why. Staying on an SSB signal as it walks several kHz across a
70 cm pass is most of what Doppler correction is for, and it was behind two switches.

The two switches were never the same kind of thing, and only one of them is kept as a
precondition:

- **The downlink is automatic.** Correcting the receive dial cannot transmit. The worst
  case is that you do not hear the bird, so it asks for nothing.
- **The uplink is still confirmed, once per radio.** A wrong VFO mapping transmits on
  your own downlink — into the satellite's output passband, on top of everyone working
  the bird — so nothing reaches your transmit VFO until you have said which VFO carries
  the uplink on the radio in use.

The confirmation now happens where you already are. On the pass readiness rail, the
Doppler row reads what your radio can do and offers it in plain words — "Confirm the
uplink and Doppler drives IC-9700 as Main = downlink, Sub = uplink" — with one button and
the mapping list beside it if the derived answer is wrong. Confirm it once and that radio
is never asked again.

Where the answer is not certain, Nexus asks instead of guessing. An IC-9700, IC-910,
IC-9100 or IC-905 running on Nexus's own CI-V connection has exactly one full-duplex
layout it can express, and that is the one offered. A full-duplex radio Nexus drives
through Hamlib, an FT-847, FT-736R, TS-2000 or TS-790, or a radio it cannot identify gets
the question, not a pre-filled answer. A single-VFO radio is offered nothing at all: its
downlink is corrected and the transmit dial stays yours.

The confirmation is recorded per radio because the satellite path routes. A pass can hand
the QSY to whichever rig covers the band, and peg-lock or a mid-pass handoff can change
which radio is under the split — so the uplink is driven only on a radio you confirmed,
re-checked every correction rather than assumed at arm time.

**If you were already correcting the uplink — Satellite Doppler on, a VFO mapping set —
nothing changes for you.** The mapping is kept exactly as written, it is not re-derived,
and you are not asked to confirm it again on any radio your station had when you
upgraded: the old station-wide grant is recorded as a confirmation for each of those
radios, and it survives every save and relaunch. A radio you add later gets its own
one-time confirmation, like any second radio. If you had picked a mapping but never
turned the old Satellite Doppler switch on, that pair never tuned anything — so the
mapping is kept, your downlink now corrects automatically, and the transmit VFO waits for
the same one-per-radio confirmation a fresh install gets. Nothing reaches a transmit VFO
on upgrade that was not already being driven before it.

**Satellite Doppler is still a switch you can turn off** — Settings ▸ Radio ▸ Satellite
Doppler — and off still means off, both legs, no dial and no split. What is gone is
having it off by default. Non-satellite stations are untouched either way: the correction
runs only inside a pass you armed, on a transponder you are holding, on a dial that pass
owns.

The pass rail now says which legs are actually being driven, separately — downlink
corrected with the transmit VFO still yours reads as exactly that, and the Doppler
readout shows an uplink frequency only when there is an uplink being written. That
honesty extends to the birds with nothing to split: a one-channel (simplex) bird rides
one dial and the rail says so instead of offering an uplink confirmation the pass cannot
use, and a beacon — downlink only — never puts anything on the transmit VFO. A radio you
remove takes its uplink confirmation with it (a replacement radio starts unconfirmed),
config profiles carry neither the VFO mapping nor the confirmations between stations,
and an operator who answers "Downlink only" is not asked again.

The confirmation itself has one writer. Picking a mapping — on the pass rail or in
Settings ▸ Radio — applies immediately and is recorded for the radio in play at that
moment; a Settings window left open across radio changes cannot re-point the mapping or
revive a removed radio's confirmation when you later press Save. When your chosen
mapping is not yet confirmed for the radio under the split — a second rig, or an
upgraded uplink-only station — the rail's confirm button offers exactly that mapping for
exactly that radio; it never swaps in a derived one over your choice. And every surface
that names who owns a frequency keys on what is actually driven: under an uplink-only
mapping the tracking badge, the rotor strip and the end-of-pass notice say the transmit
(split) VFO is Doppler's and the dial stays yours, instead of claiming a dial that never
moved.

### Known limitations

- Multi-window per-radio profiles keep their files under a directory named by the radio's
  internal id, and ids are reused: remove a radio and add another, and the new radio can
  inherit the removed radio's window settings, geometry and journals. Pre-existing and
  unchanged in this release — recorded here so it is not mistaken for a new fault. The
  uplink confirmation is NOT affected: it is pruned when a radio is removed, so a
  replacement radio always starts unconfirmed.

## [0.25.0] — 2026-08-01

### Fixed: torn-off windows open at a readable size

Pop out the CW band map and the type was tiny — and stayed tiny at every size the window
could be dragged to, docked or free. Every torn-off window sized itself by asking whether
the full Operate cockpit fit inside it. A 420-pixel-wide band-map strip never fits a
cockpit, so the answer was always "no" and every pop-out opened at the smallest scale the
app has, permanently.

Each pop-out is now measured against its own contents instead. The band map, the
waterfall strip, the Needed board, Connect, DXpeditions, Memories, Satellites and the
Field Day scoreboard all open at full size, and shrink only when you actually squash the
window down toward its minimum — where shrinking is the right answer. The Operate pop-out
is unchanged: it really does host the cockpit, so the cockpit is the right question for
it. The main window is unchanged at every size.

If you have pinned a scale in Settings, that pin now applies in a pop-out as far as the
window can take it, instead of being crushed to the smallest step. A pin is still never
rewritten by a pop-out, and still never enlarged past what the window can show.

The Needed board no longer carries its 25% font compensation — it existed only to offset
the scale bug, and the board now reads larger than before without it.

### Added: the satellite catalog ships with the app

A new install used to start with nothing and go looking for orbital elements. If the
element mirror could not be reached, the fallback fetch brought back CelesTrak's
`amateur` group — 97 objects, no status information — so the Satellites screen listed a
fraction of the birds and could not say a word about any of them: nothing marked dead,
re-entered or not yet launched, nothing marked alive but silent, no row explaining a
bird it holds no elements for, and a starred bird that stopped working simply vanished
from the list with the star still set.

The installer now carries a snapshot of the full catalog — 430 birds, 367 of them with
elements — so the Satellites screen is complete from the first launch, with no network
at all. The snapshot is a floor, never a ceiling: it fills in what your copy is missing
and never replaces newer elements with its own, never overwrites the statuses you
already have, and never counts as a fetch, so Nexus still refreshes from the mirror at
the first opportunity exactly as before. Upgrading from an earlier version keeps
everything already cached and gains the catalog beside it. Elements from the snapshot
age like any other: past 30 days they stop being used, because SGP4 accuracy is gone by
then and a position would be a guess.

Satellite population, names and status are derived from the SatNOGS DB (CC BY-SA 4.0);
orbital elements are courtesy of CelesTrak and the SatNOGS TLE API.

### Fixed: SSTV receives pictures without being told to, and says what it is hearing

You could hear a picture coming in on the speaker, watch it march up the waterfall, and
still get nothing — no image, no progress, no explanation. Opening the SSTV screen now
starts the receiver. Until now it had to be armed by hand every session, and until it
was, the decoder was handed no audio at all, so the ordinary way to use SSTV (open the
screen, tune 14.230, wait) decoded nothing. The waterfall and the decoder are fed by two
different paths, which is why a live waterfall never proved the decoder was being fed —
and why the failure was invisible. Stopping the receiver yourself is still respected: it
stays stopped for the rest of the session, and nothing here can transmit.

The line under the waterfall now states what the receiver actually hears rather than
one fixed hint. It tells apart a stopped receiver, a capture device delivering nothing,
an input that is alive but silent, a picture in progress, and — new — a station sending
in a mode Nexus cannot decode, which previously looked exactly like a dead band because
it was only ever written to a console log. Hearing the signal on the speaker says
nothing about what the app is capturing, and the screen now says so instead of leaving
you to guess.

**More SSTV frequencies, and the right one.** The band list gained 160, 17, 12 and 6 m,
the European 40 m calling frequency, and the 20 m overflow channels 14.233 and 14.236 —
the last of which is where the original report came from, and which existed nowhere in
the app. The on-screen hint now names the calling frequency for the band you are on
instead of reciting 14.230 and 145.800 whatever you are tuned to. While a picture is
coming in, the caption says how long that mode takes on the air, so a 110-second
Scottie 1 no longer looks like a hang.

Three more things that came out of reviewing the fix. An ISS pass ending no longer
leaves SSTV switched off for the rest of the session — the automatic disarm at loss of
signal was indistinguishable from you pressing Stop, so anyone using the ISS auto-arm
option would have lost SSTV decoding from their first pass of the day onward. The new
status line no longer blames your sound card in the second before the decoder has
reported anything; until it has heard something it says so plainly. And 28.680 is no
longer described as Technician-accessible: US Technicians have 10 m images only on
28.300–28.500.

### Fixed: the map stopped throwing away its canvas every second

On a wide display Nexus's memory use oscillated by ~20 MB once a second — visible in
Task Manager as a steady sawtooth. The map's draw pass reassigned the canvas size on
every run, and assigning that size discards the whole image buffer and allocates a
fresh one even when nothing about the size changed. Since the draw pass runs on a
one-second pulse whenever an animated layer is showing (band openings and the heat
layer are on by default), a full-window buffer — about 20 MB at 3440x1440 — was
thrown away and rebuilt every tick. Nothing leaked; the memory was reclaimed each
time. But it was work for no result, and on a slower machine that kind of churn is
felt as stutter rather than seen in a graph.

The map now resizes its canvas only when the size genuinely changes, matching what
the waterfall has always done. The flare overlay got the same treatment (it was
rebuilding a full-window buffer on every zoom and drag), as did the small spectrum
display in Connect and Settings, which was doing it eight times a second. A test now
fails the build if any drawing surface goes back to resizing on every frame.

### Added: satellite operating without the box — and passes that know their worth

**No rotator? Tracking still works.** Arming a pass no longer needs a rotator: the pass clock, the
sky dome and Doppler all run, and only the pointing is skipped — the handheld-antenna operator
gets everything but the mast, including where to swing the antenna. The tracking badge, the
readiness rail and the rotor strip say exactly which surfaces a track actually drives — rotor,
dial, both, or neither — and a track driving neither says "pass timing only" instead of implying
more. While a rotor-less pass holds the dial, an ownership chip appears in the cockpit header
naming the bird, with a stop button that hands the dial straight back; stopping a live track
always releases the dial.

**The uplink's sideband is set with its frequency.** On an inverting bird you listen USB and
transmit LSB. Nexus now puts the TX (split) VFO in the right sideband along with its frequency
while a pass owns the uplink — commanded once per answer, only when the two legs genuinely differ,
and only onto the satellite's own uplink: a terrestrial pile-up split worked while a transponder
is held keeps its own sideband. Reach for the rig's mode knob yourself and Nexus stands down for
the rest of the pass rather than fighting you. If the rig refuses the command, the status line
says which sideband to set by hand — a wrong uplink sideband sounds exactly like nobody answering.
The Doppler readout and the transponder chooser show what is actually being commanded, and say so
plainly when nothing is.

**Passes ranked by what they would earn you.** The schedule and the "Next up" strip now say what
each pass is worth: the grids you have never worked via satellite and the entities you have never
worked at all, wherever the pass's footprint crosses them — in the same need-chip language as the
Needed board, with the sample squares on the pass timeline. "Needed" is a column you click to sort
by; the default order stays soonest-first, and a bird SatNOGS reports dead still shows its dead
tag here. Satellite VUCC is now its own card on the Awards screen — ARRL counts a satellite
contact toward that category only — so the terrestrial VUCC card counts terrestrial grids, and a
satellite QSO no longer silences a NEW GRID call-out for a band slot that is genuinely still open.

**One click to work a pass.** ▶ Work this pass opens the bird, picks a workable transponder for
you — never a beacon, never one reported dead, and never overriding a "None — leave the dial to
me" you said for that bird — and arms the track. A readiness rail under the bird's name shows the
whole chain (pass, rotor, transponder, Doppler), each gate fixable where you are standing. The
transponder chooser is a card list beside the tuning instruments with dead entries folded behind
one line, and the sky dome, timeline, chooser and passband strip sit together above the globe.
What the rail and chooser show is what the engine actually holds: when a pass ends or is stopped,
the hold is released and the display follows.

**Picking a transponder tunes the radio.** Choosing a bird — by hand or through ▶ Work this pass —
puts the rig on that transponder's downlink there and then, with the uplink on the transmit VFO
your mapping calls for. No waiting for AOS: the pass takes over from where the pick left the radio.
The click is consent for the dial only, exactly as clicking a spot or a repeater favourite already
is; every transmit gate is unchanged, and "None — leave the dial to me" still means the radio does
not move. If Doppler is switched off, your mapping is None, or the rig cannot reach the band, the
transponder is still held and the section tells you plainly that nothing was tuned.

**The Satellites section says which radio it drives.** A dual-radio station no longer has to guess.
Picking a transponder routes on band and mode class the same way a repeater tune does, so a
VHF/UHF bird reaches the VHF/UHF rig even when you were sitting on HF, and an FM bird follows your
FM routing rule while a linear bird follows your SSB one. A line under the held bird's name names
the rig, the band and the class it routed on, and the frequencies it actually wrote — with a pin
to hold the current radio when you want to override the routing.

**A routing rule can now say "Satellite".** Asked for from the field: mode-class routing sends a
packet bird through your FM & APRS rule — right by the rules, wrong for the shack where APRS
lives on one rig and the satellite antennas on another. The rule editor's mode dropdown gains
**Satellite**: a rule so designated is checked before the mode rules, but only when a satellite
tune is asking — picking a transponder finds it, a terrestrial FM, APRS or FT8 tune never does.
Without a Satellite rule nothing changes: satellite picks keep routing on band and mode class
exactly as before. One caution: builds before this one don't know the designation and read a
Satellite rule as a plain terrestrial rule — with no band or mode set, one that catches every
tune — so if you ever roll back, delete the Satellite rule first.

**No more FT8 frequencies under Satellites.** The band dropdown at the top of the window is fed the
digital watering holes, which meant it offered 14.074 beside a bird on 435 MHz. Satellites now owns
its own frequency surfaces, like Phone, CW, RTTY, SSTV and APRS already do.

**Orbital elements stay current — and say so when they can't.** Elements now refresh in the
background from the project's mirror of CelesTrak's amateur list, held in one shared on-disk
snapshot: no more re-fetch on every launch, nothing ever waits on the network, and CelesTrak
itself is asked directly only as a narrow, rate-respecting fallback. The stale badge is now a
refresh button, and Settings gains an Orbital elements block with **Update now** and **Import
from file** — the path for offline shacks and brand-new launches. Update now means now: if the
mirror can't deliver when you press it, the same attempt goes straight to CelesTrak instead of
telling you to come back tomorrow — still honoring CelesTrak's update cycle, and never after
CelesTrak has said stop. And the answer speaks operator, not protocol: a fetch that fell through
to CelesTrak says so with the bird count, an unreachable mirror over current elements says your
elements are fine (the mirror goes live with the next site release), and a real failure says
what failed and what to do next — no more "HTTP 404" as the answer to a button press, with the
raw error riding the tooltip for troubleshooting. Element age is enforced where it matters: past
14 days arming a pass asks first; past 30 days the detail pane, tracking and the
SSTV auto-arm refuse plainly, naming the bird and the age, instead of pointing the antenna with a
fiction. A pass keeps the elements it armed with (their age shown on the readiness rail), a bad
or empty download never replaces a good cache, and a CelesTrak rename no longer orphans your
starred birds, alarms or schedule — Nexus remembers the catalog number behind each name.

**Connect's satellite layer follows your stars.** Turning on satellites in Connect now shows the
birds you have starred in the Satellites section — on the 2-D map, the 3-D globe and the Passes
pane alike — instead of the whole amateur catalog. A small ★/All chip flips between your birds
and everything — on the Passes pane and beside Satellites in the map and globe Layers panels —
one choice all three surfaces share and apply the moment you flip it, and with no stars set
the full sky still shows so a fresh install is never empty. If every starred bird has dropped
out of the current elements, the map and globe say so rather than drawing a silently blank
sky. Every bird on the map and globe now
carries its designation ("ISS", "RS-44") beside its satellite icon, and map labels that would
overprint each other in a cluster shuffle apart so both names stay readable. Stars recognized by
catalog number too, so an upstream rename never drops a bird from your filtered sky.

**The bird list is the birds that are actually up there — and it says when one stops being one.**
Nexus used to show one Celestrak list of 97 objects, and measured against AMSAT's live reports
roughly 60 of those had not been heard in a month, while eight birds heard on the air that same
day were missing from it entirely (IO-86, Foresail-1p, SAKHACUBE, QMR-KWT 2, Ten-Koh 2, Marina and
two more). The list is now built the other way round: start from the satellites the SatNOGS
database says carry an amateur transmitter, then go and find current orbital elements for the ones
that are actually workable — from the two Celestrak groups and the SatNOGS element service
together. That is **430 birds listed and 367 with live elements**, against 97 before: 274 of them
are birds the old Celestrak group never carried at all. Birds catalogued under a placeholder
number until Space Command assigns a real one (normal for the first months after launch) now
appear under one entry instead of two or none.

Every bird carries its status with it, so the Birds list, the map and the globe can show what only
the Satellites schedule could show before — and a bird that stops being workable keeps its row and
says why: **dead**, **re-entered**, **pre-launch**, or **alive but silent** when nothing amateur is
transmitting on it any more. A starred bird that dies no longer disappears out from under your ★;
it stays in the list, wearing the reason, until you take the star off yourself. Birds nobody holds
current elements for are reported the same way — no elements at all, elements too old to trust, or
an orbit decayed past the point where a position can be computed. The Connect Passes pane names
those birds under the passes, and searching the Birds list reaches the whole catalog, elements or
not, so a bird you unstar is always a bird you can find again. A bird that re-entered more than
six months ago drops off: by then it is history, not a catalog.

**And your first ★ birds are set for you — once.** A fresh install used to open Satellites on an
empty schedule beside a list of hundreds of birds, with nothing to say which are worth chasing.
Nexus now stars the ten most workable active birds over your grid the first time it can: alive,
carrying a live amateur transmitter, with current elements, and flying real passes over you —
ranked by how many of those passes you get and how high they climb. A line at the top of the
section says it happened and disappears when you dismiss it, and every star it set is one you turn
off where you are standing. It happens once. Clear the whole set and you get an empty sky, not the
same ten back next launch; a station that already has favourites is never seeded over, and neither
is one that deliberately cleared theirs — including one that cleared them on an older version.
With no grid square set, or before the bird catalog has landed, nothing is starred at all: a guess
is worse than waiting.

The mirror that feeds all this refreshes every six hours and refuses to publish at all if any of
its sanity checks fail, so a bad upstream day leaves your last good list in place rather than
replacing it with a short one. If the mirror itself is unreachable for a day, Nexus falls back to
fetching Celestrak's amateur group directly — a shorter list, and every bird it does not carry
keeps its row, marked as having no current elements.

### Fixed: the pick reaches the radio — and the IC-9700's uplink goes where it can transmit

**A dual-radio pick finds the satellite rig even with nothing configured.** Field report: on an
FTdx10 + IC-9700 station, clicking a transponder moved nothing — no routing rule named the 9700
for the band, neither rig listed bands, and the pick stopped at "this radio doesn't cover
435 MHz" while the 9700 sat idle. When no rule or band list answers but the active rig is known
unable to reach the downlink and exactly one other radio exists, the pick now hands the QSY to
that radio. With several candidates and nothing naming one, the section refuses and says exactly
what to configure instead of guessing which rig to move. A radio whose band list explicitly
leaves the downlink's band out is never the fallback — that list is your word that the rig
doesn't take the band, and the pick refuses rather than switching to it just to be turned down.

**A packet bird is FM to the radio — and routes like it.** Field report: on the same station,
picking the ISS APRS digipeater (145.825 up *and* down) selected the HF rig and called the bird
SSB. The mode class behind routing recognised only the literal names "FM"/"FMN", so every packet
mode SatNOGS uses — AFSK, FSK, GMSK and the rest of the family — fell through to the SSB class:
the FM routing rule never matched, and USB would have been commanded on an FM channel. One
mode-name map now classifies the whole packet family as FM everywhere the class is consulted —
routing, the commanded rig mode, and the uplink's sideband — so the digipeater reaches the
VHF/UHF rig, in FM. Linear birds (the RS-44 class) behave exactly as before, and a mode name the
map has never seen still reads SSB, as it always did.

**A simplex bird rides one dial.** 145.825 up and down is one channel, not a cross-band pair: the
pick no longer writes a split for it, nothing engages the rig's satellite mode, and mid-pass
Doppler holds the dial on the published frequency — the two legs' corrections are equal and
opposite, so steering the one dial to either leg lands the other twice as far off, outside an FM
passband. Parking on the channel is what every 145.825 operator does by hand, and now the tracker
does the same. The radio line shows the one frequency once, and the Doppler row says the two legs
share the dial instead of printing the VFO mapping twice. Cross-band FM channels (SO-50's
145.850 ↑ / 436.795 ↓) keep both dials steered as before.

**Cross-band uplink on the IC-9700 rides satellite mode.** The uplink used to be written as an
A/B split — which on a dual-band rig lands in the *downlink's own band* and goes nowhere. Under a
Main = downlink / Sub = uplink mapping, Nexus now engages the rig's satellite mode, writes the
uplink (and its sideband, on an inverting bird) into the Sub band with a read-back check, and
hands the tuning selection straight back to Main, so the dial, the scope and every poll keep the
downlink. Releasing the split releases satellite mode; switch it off on the front panel yourself
and Nexus reports that instead of re-engaging over you. A rig without a Sub band refuses the
mapping honestly, Main = uplink / Sub = downlink is refused as undrivable (satellite mode always
transmits on Sub), and A/B rigs keep the existing split behaviour byte for byte. An ordinary A/B
split commanded while satellite mode is engaged — WSJT-X setting up split for a digital over
mid-pass — releases satellite mode first, so its transmit dial can never land in the Sub band
and go out on the downlink. And the sequence trusts nothing it cannot confirm: an engage whose
confirming read-back is lost is backed out rather than left half-set, a hand-back to Main the
rig refuses is remembered and re-asserted before the next tuning write or dial poll, and a rig
that will not leave satellite mode is reported in the status line instead of being shown as
simplex.

**The radio line reports what the rig acknowledged, not what was computed.** The binding under the
held bird used to print both frequencies the moment you clicked — including when nothing had been
sent. Each leg now shows as still tuning ("435.640 ↓ …") until the radio actually accepts it, the
dot fills only when every requested leg is confirmed on the wire, and a leg the rig refuses turns
into the reason in plain words. A refused pick no longer flashes a green "Working …" toast — the
toast tells the truth the read-back found.

**Clicking a transponder no longer freezes the app (0.24.3 tester build).** Field report: on the
IC-9700 under the native CI-V daemon, picking a bird and clicking a transponder frequency froze
the whole window until Windows killed it, every time. The radio loop deadlocked on itself while
applying the uplink split — it took the engine's own lock a second time while still holding it —
and every part of the UI then queued behind that lock forever. The same wedge caught terrestrial
pile-up splits ("UP 5" spots) and every mid-pass Doppler correction. The apply now releases the
lock before talking to the rig, and a liveness test drives the real pick against a simulated
9700 under a watchdog so this class of freeze fails the build instead of the operator. And
because the rig conversation now runs unlocked, a split the operator requests during it is
safe: a rejection names the dial it was rejecting and resolves only that request, so a fresh
"UP 5" click in that window still applies on the next cycle instead of being silently dropped.

### Fixed: RTTY no longer freezes the window when the copy gets rough

**Field report: "getting some application hangs on RTTY."** The decoded-text pane fades each
character by how confidently the demodulator copied it. That confidence is a continuous
measurement, so on clean copy the whole transcript was one solid block — but as conditions
degraded it crossed the fade thresholds constantly, and the pane ended up drawing the transcript
one character at a time: up to four thousand separate pieces, redrawn twice a second, forever,
because the transcript keeps scrolling. Bench-testing on a strong signal could never show it; a
noisy band or an armed decoder listening to empty air brought it on, which is why it came and
went. The pane now draws at most a couple of hundred pieces no matter how bad the copy is — the
same "cap the feed" rule the decode history already follows. Every decoded character still
prints, and the fade is still scored character by character for any transcript under that
ceiling, which is every transcript you can actually read. Only when the copy is breaking up
badly enough to blow the ceiling does the fade get scored over short stretches instead of single
characters; from that point a brief marginal burst can be averaged in with the good copy around
it and shown solid. That is a deliberate trade, and it is only ever charged on copy already too
broken to trust.

**A stalled radio can no longer stop the window from responding.** Nexus talks to the rig over
a serial link that can take up to 2.5 seconds to answer, and while it waits it holds the lock
that the rest of the app needs to read anything. Those reads were running on the same thread
Windows uses to paint and respond to clicks, so a slow or wedged rig could park the window long
enough for Windows to declare it "not responding" — with nothing in any log, since nothing had
actually crashed. Those reads now run off the painting thread: a slow rig costs a late reading,
not a dead window. The long jobs got stronger treatment — a LoTW, eQSL or QRZ sync can sit on the
network for a minute, and those now run on a pool set aside for waiting, so a slow QSL server
can't hold up the rest of the app either. A build-time check keeps it that way; it is keyed on
which commands can reach the shared radio state, so a new one can't slip through by reaching for
it in a different way.

**One misbehaving companion app can no longer freeze the radio loop.** A logger that jams the
WSJT-X control port with stop-transmit commands was previously handled all in one pass, each one
costing a full round trip to the rig — with the app-wide lock held the whole time. Nexus now
handles a bounded batch per cycle; the rest waits for the next cycle a few milliseconds later,
so nothing is dropped.

**And the class of bug behind the recent satellite-picker hang is now caught at build time.**
That freeze came from a Rust pattern where a lock is accidentally kept open longer than it
looks — invisible in review, with no symptom until the app is already frozen. The build now
rejects the pattern in the command layer where it does the damage, and flags it everywhere else.
Two spots in the FlexRadio streaming code that matched it were rewritten (both harmless today,
neither harmless to leave).

### Fixed: the Openings Log square works — and a broken screen can no longer black out the app

**Turning on the Openings Log square blacked out the whole app (0.24.6 tester build).** Field
report: assigning "Openings Log" to a Connect slot turned the entire window black — rail, top
bar, everything — and the black screen came back on every launch, because both the slot
assignment and the openings journal itself outlive the session (the journal even outlives a
wiped browser profile). The pane paints in two steps — an empty first paint while the log
loads, then the real render — and its sort state was declared on the wrong side of the
"nothing yet" exit, so the two paints disagreed about the component's shape and React tore
down the entire window. The sort state now sits above that exit, the pane renders whatever the
journal holds — including rows written by older builds with fields missing — and a build-time
sweep now fails on this pattern anywhere in the UI, so the class is closed, not just this
instance.

**A broken screen can no longer black out the app.** A section that crashes while drawing now
shows an error panel in its place — what crashed, the technical details ready to copy into a
bug report, and a button back to a known-good section — while the navigation rail and top bar
stay alive around it. Pop-out windows carry the same net. And the section the app reopens on
is checked against what this build can actually render before it is restored, so a saved id
from an older or newer version can never wedge startup into a dead screen.
### Added

- **The recall card's picture opens their QRZ page.** Click the callbook photo on the
  station you are working — or the initials circle when they have no photo — and QRZ opens
  in your browser, so you can read the page while you are still in the QSO. The circle is
  a link only once the callbook has actually resolved the call; before that it is the same
  plain badge it always was. Clicking it does not move the cursor out of the log field you
  are part-way through typing, and it is reachable from the keyboard like any other control.

- **CQ+73 filter chip** in Band Activity (a tester request): the CQ view plus RR73/73
  signoffs — a signoff means that frequency is about to free up. Sits between CQ and
  To me; the plain CQ chip is unchanged. Detection rides the message parser, so a
  `DM73` grid or an `RRR` roger never counts as a signoff.

### Fixed

- **TX Off follows WSJT-X's Enable-Tx contract in the FT modes.** Turning TX Off lets a
  transmission already in flight finish its over, and the next cycle simply doesn't start;
  turning it back on before the next cycle transmits normally. Stop TX is unchanged — it
  halts immediately, mid-over. Previously both controls cut the transmission on the spot.
  Only the FT-style cycle gets to finish: in CW, RTTY, SSTV, the voice keyer and APRS,
  TX Off still stops the over on the spot, and the cockpit Stop buttons now unkey even in
  the last fraction of a second of an over instead of leaving the rig keyed to the tail.

- **The Band Activity filter chips wrap onto a second line instead of running off the edge
  of a narrow rail.** The chip row could not wrap, which made it a single unbreakable
  block: with the rail dragged to its narrowest the last chips sat past the panel edge,
  and nothing in that column scrolls sideways, so there was no way to reach them. Adding a
  seventh chip turned that from marginal into certain.

## [0.24.0] — 2026-07-31

### Added: satellite operating — full Doppler, and a rotator that behaves

Nexus now works a satellite pass end to end. Your rotator, a standard computer interface, and
Nexus — no separate tracking appliance.

**Doppler on both legs, continuously.** Your downlink is corrected so you hear the bird where it
actually is, and your uplink is corrected so the bird hears *you* where it is listening. Both,
always: correcting only the downlink sounds right while your signal slides off the far end of the
passband and nobody comes back to you.

**Inverting transponders are handled properly.** On an RS-44 or AO-7 the passband is mirrored —
tune up the band and your uplink goes down, and the sidebands swap. That comes from the satellite
database per transponder, not from a checkbox you have to remember, so the common way to land on
somebody else's QSO is closed by default.

**Tune the downlink and the uplink follows.** Chase a station drifting through the passband the way
you would on HF — turn the VFO knob and your transmit frequency tracks them, mirrored if the
transponder inverts, while Doppler moves the whole band underneath. Tuning outside the passband is
treated as what it is, leaving the transponder, so a QSY away from the pass never drags your uplink
to a passband edge.

**You can see where you are in the passband.** A strip shows the transponder as a band with your
position marked on both legs at once. On an inverting transponder the two markers sit on opposite
sides of centre and move in opposite directions as you tune — which is the clearest way to learn the
one rule that catches everybody out, and no other display draws it. Birds with no passband to tune
inside, like the FM repeater satellites, say so instead of drawing an axis that does not exist.

**Digital modes are handled honestly.** For slot-timed modes like FT8 the dial is held still for the
length of a transmission and re-corrected between overs, because stepping the frequency underneath a
transmission in progress smears it. The physical Doppler on a fast low-orbit pass still applies and
no software can remove it, so FT modes remain best suited to the high-orbit birds. SSB, CW and FM
steer continuously, which is what you want.

**Rotator control worth using.** Park and ready positions with a post-pass action, an optional flip
above 90° elevation so a high pass goes over the top instead of spinning the mast at zenith, an
az/el deadband so the rotator stops hunting for ten minutes straight, and calibration trim for the
difference between where the controller thinks it points and where the boom actually points.

Nothing moves until you say so: Doppler is off until enabled, the VFO mapping starts at "off" until
you say which VFO carries which leg, flip is off, and the post-pass action leaves the antenna where
the pass ended.

**Satellite contacts count.** A QSO logged during a pass carries SAT_NAME and PROP_MODE, which is
exactly what LoTW needs to credit it.

**You can see the pass, not just read it.** The sky dome is now the main view while a pass is
running — a proper polar plot with labelled elevation rings and the track weighted so the high,
workable part of the pass reads first. The satellite moves along it as the pass runs.

Beside the satellite, Nexus draws where your *antenna* was actually pointed, and states the gap
between the two as a single angle. Every other display prints satellite position and antenna
position as two rows of numbers and leaves you to do the subtraction, which matters because a
deadband means the two legitimately differ by a degree or two — knowing that is normal is the
difference between trusting the rotator and chasing a fault that is not there. If your rotator is
azimuth-only, Nexus draws an azimuth line and says in words that no elevation was sent, rather than
drawing an antenna lying on the horizon. Before the pass, when auto-track is deliberately keeping
its hands off your rotator, it draws nothing at all and tells you the azimuth the bird will rise at.

**The 3-D globe becomes the pass view.** When a pass is tracked the globe frames your station and
the satellite together, draws the orbit behind and ahead, the footprint, and a line of sight from
your antenna to the bird — the range figure, drawn. The flat map stays the "where is everything"
view. Neither shows anything before the satellite is above your horizon.

The Doppler readout shows both legs with their live frequency and correction, and both views carry
a written description for screen readers.


### Fixed: Test CAT now root-causes the Icom "answers nothing" failure

An IC-7610 (or IC-9700/7300/705) that never answers CAT — "isn't answering (got \"\")" in both
normal and native CI-V mode — is almost never a broken radio. It is one of two setup traps, and
Test CAT now tells you which one you are in:

- **Wrong baud.** When the configured rate gets silence on a serial Icom, Test CAT now re-probes
  the same COM port directly at every rate the rig's CI-V menu offers (19200 / 9600 / 4800 /
  38400 / 57600 / 115200, read-only — it only ever asks for the dial frequency). If the rig answers at another rate, the
  result says so and gives you both fixes: change Baud in Settings, or set the rig itself —
  MENU » SET » Connectors » CI-V » "CI-V USB Baud Rate", with "CI-V USB Port" = "Unlink from
  [REMOTE]". That last menu is the usual culprit: from the factory the USB CI-V port is linked to
  the [REMOTE] jack and tops out at 19200, so the 115200 the native scope needs gets you nothing
  at all, not even garbage.
- **Wrong COM port.** The IC-7610 and IC-9700 present TWO COM ports and only one speaks CI-V.
  If no rate answers, Test CAT now walks you through telling them apart (Device Manager: the
  CP210x port marked "Enhanced" — Icom's driver calls it "Serial Port A (CI-V)"), and Detect /
  the setup wizard / port Auto-test now label and prefer the CI-V side of the pair instead of
  showing two identical "Icom IC-7610" rows. On the single-port models (IC-7300/705/905) the
  silent-at-every-rate verdict instead checks that the chosen COM port is really the rig.

Test CAT also now says **which backend it exercised** — native CI-V or Hamlib rigctld — in green
and red results alike, including when the native daemon failed to start and CAT quietly fell back
to Hamlib (previously that fallback was invisible, so you could spend an evening debugging the
wrong one). And the result you read is the result of the probe you clicked: the button used to
report a stale status whenever the rig rebuild took longer than its fixed wait.

### Changed: the SSTV screen — a big crisp picture, and panes that take only what they need

A decoding SSTV picture now grows to fit the window in exact whole steps of its native
resolution (up to 6×), so a big monitor shows a big, still-crisp image instead of a postage
stamp surrounded by dead space — and it steps down cleanly on small windows instead of
spilling out of its area. Below the picture, the Transmit box now takes only the height it
needs and the Gallery gets everything left over; the old layout gave each exactly half the
space whatever they held, which left a fresh install staring at a large empty Gallery band.
Both panes carry the same title-bar frame as the other screens, and the ⊞ Panels menu hides
them exactly as before. On a window too short for all of it, the screen scrolls: the picture
area and the Gallery keep a usable minimum instead of vanishing, and the Send/Stop bar sits
below the panes and stays parked at the bottom edge — Stop is reachable at any scroll
position. RTTY's decoded-text pane keeps its minimum height on short windows the same way.

### Changed: the full caller card is back while operating — QRZ photo, bearing and all

Resolving a call in the Phone or CW log strip once again shows the full recall card: the QRZ
profile photo, name, QTH, country, the distance and beam heading from your own grid, your most
recent note on the station, and the real list of previous contacts (date, band, mode, reports).
The card had been cut to a single line in 0.18.0 because it could push the operating controls
off screen; the new pane layout scrolls the log column internally, so the card can take its
full height without squeezing anything. A long history scrolls inside the card rather than
growing it.


### Fixed: satellite predictions were off by a second of orbital motion

Every satellite prediction — Doppler, pass times, look angles, the ground track — was computed
from an element-set epoch rounded down to the nearest whole second. A TLE states its epoch to
sub-millisecond precision, and the fraction that was being discarded is effectively random from
one element set to the next. A low-orbit satellite travels about 7.6 km in the second that was
being thrown away.

In practice that put the bird up to several kilometres from where it actually was, and moved the
Doppler correction by up to about 75 Hz on 70 cm — enough to sit noticeably off a narrow CW or SSB
signal on a fast pass, and over 1.5 kHz on a 10 GHz downlink. Pass rise and set times were off by
up to a second for the same reason.

The epoch is now kept at full precision. Nothing about how you operate changes; the numbers are
simply right.

This was found by a new test that checks predicted Doppler against carriers actually recorded off
the air by volunteer ground stations, rather than against another copy of the same theory. The
existing cross-check against an independent implementation had missed it, because its reference
data had been generated with the same rounding and the two errors cancelled — which is exactly the
blind spot that testing against real recorded signals exists to close. That reference has been
regenerated correctly, and agreement on the quantity that reaches the radio improved by 40%.

### Fixed: the propagation advisor stops changing its mind on identical data

On a quiet band with one or two spots — exactly when you lean on it — the advisor's "best
region", the beam heading, and the confidence word could flip between polls with nothing
changed on the air, and when two openings tied, which one got the single "just opened — jump
on it now" alert was re-rolled every poll. Ties are now broken deterministically everywhere
one reaches the screen, so identical data always gives the same answer. The same fix covers
the Journey "most QSOs in a day" date, which station leaves a full APRS list, and the link
readout's frequency on equal-strength decodes.

### Fixed: the torn-off band map disagreed with the docked one about who you need

On a pop-out band map or a second-monitor Connect map, a station that is an all-time-new
entity on the band in front of you could be painted in the dim "needs a confirmation" colour
because it also wants a QSL on another band — while the docked window showed it correctly.
Both windows now colour from the same shared logic, and a test pins the rule.

### Fixed: the 3-D globe's open-band glow was frozen

The globe's band heat updated its "breathing" only once a minute, so an open band could sit
dimmer than a closed one and the opening wedges never pulsed — on the 2-D map, that motion is
how you tell a live opening from stale spots at a glance. The globe now breathes exactly like
the map, from one shared clock, and stays still only when nothing is open (or the tab is
hidden).

### Fixed: logging could freeze the app when HRD forwarding pointed at a slow hostname

Forwarding a logged QSO to Ham Radio Deluxe resolved the target hostname while holding the
app's main lock — with DNS slow or down, every logged contact froze the interface for the
timeout and could cost the transmit slot right after logging. The send now runs off-thread,
like the N1MM forwarder always has.

### Fixed: watch-list stations vanished from a filtered Needed board

Turning on any Needed-board filter hid the callsigns you explicitly asked to be told about —
the watch-list tier had no filter bucket, even though it outranks everything. It has its own
chip now, and the dupe cue ("already worked this station on this band") also lights for
contacts that arrived from a LoTW or QRZ import, which a case-sensitive compare left dark.

### Fixed: the shipped AI CW decoder was on a vulnerable model loader

Turning on the supply-chain scan for the desktop build's own dependency set (it was never
scanned — only the test tree was) surfaced a known out-of-bounds read in the neural-net model
loader the AI CW decoder ships with, plus two denial-of-service advisories in an XML parser.
All three are gone: the decoder moved to the patched inference library (also ending a
situation where the tested decoder version differed from the shipped one), and the XML
parser was replaced by its patched line. CI now scans the shipped dependency set, runs the
38 propagation tests that previously ran nowhere — including the ones that prove connector
credentials never travel unencrypted — and keeps the two dependency trees aligned. Building
Nexus from source now needs Rust 1.91 (the patched inference library's own minimum).

### Fixed: every transmitting mode now refuses to key without a real callsign and grid

The blank-identity guard that has always protected FT8 and FT4 now covers every mode that can
transmit. Before this, selecting Q65, FST4, MSK144 or JT65 and pressing Call CQ with no callsign
set would key the rig and send a standard message with the identity missing — an unidentified
transmission. WSPR and FST4W beaconing now insist on a real callsign and a real grid too, not
merely a non-empty box, since those reports are published to wsprnet under whatever you typed.
The check is wired into each mode's own capability declaration, so a future mode gets the guard
the day it learns to transmit.

### Fixed: an over planned just before you changed something could still go out against the old settings

There is a short window while a transmission is being prepared in which the app used to re-check
only the mode tier before keying. Stop TX pressed in that window, a QSY to a new frequency on the
same band, a sideband or operating-mode change, or the T/R period rolling over could all slip
through, and the prepared over went out anyway — against the frequency, mode or slot you had just
left. The commit step now re-checks everything the go/no-go decision was based on and quietly
drops the over if any of it moved; the next slot plans a fresh one. The PTT hold time is also now
measured from when the audio actually starts rather than from the start of the preparation, so
the tail is never cut short.

### Fixed: one internal crash could silently kill receive — or all of TX/RX — for the whole session

A crash inside one decode used to be contained but leave the decoder's lock unusable, so every
later period failed instantly: the waterfall kept painting and the app looked alive while it had
gone completely deaf, and switching modes or opening the snapshot could freeze the UI with it. In
the worst case the radio loop thread itself stopped — with nothing left to drop PTT if it died
mid-over, and the "RADIO ENGINE STOPPED" banner unable to appear in exactly that case. All of the
app's shared state now recovers from a crashed thread instead of seizing, the radio loop drops
PTT before it ever exits on an error, and a contained decode crash now shows up as a visible
notice instead of a line in a log nobody sees.

### Fixed: an APRS beacon or auto-ack could transmit on top of your live microphone or a logger's over

With the mic keyed on FM — or WSJT-X keying the rig through Nexus's CAT broker — a queued APRS
beacon or an armed auto-ack would key anyway and lay packet audio across the transmission in
progress. Every transmit gate now asks one shared "who owns the transmitter" arbiter that knows
about all of it: a slot over, the tune carrier, your mic, a broker client's key, the voice keyer,
CW, RTTY and SSTV. APRS holds its queue until the air is actually free, and a broker client asking
to key while an SSTV image or voice message is playing is now refused too.

### Fixed: the tune carrier's auto-release could be configured into a minutes-long unattended carrier

The "Tune after t s" auto-release honoured any number typed into it, and settings.json is
hand-editable — a mistyped 120 for 12 meant a two-minute continuous carrier into your finals or a
dead load with nothing to catch it. The auto-release is now capped at 60 seconds however the
setting got its value; deliberate longer settings up to that minute still work.

### Fixed: one malformed APRS packet or log record could kill APRS receive or the whole radio session

A single APRS position report carrying an 8-bit character — a latin-1 degree sign from a real
tracker or an APRS-IS feed is enough — crashed the APRS receive thread, and the station list
simply stopped updating for the session with no sign anything was wrong. Roughly one in ten
corrupted position packets could do it. Separately, an ADIF record whose end-of-contact time
carried a multibyte character — from a hand-edited file, another logger's export, or a bad
WSJT-X logging datagram — could crash the radio loop itself, ending TX/RX until restart. Both
parsers now treat malformed input as the one bad packet or record it is and carry on.

### Fixed: Tempo QSOs no longer lose their protocol identity when the app restarts

A TempoFast or TempoDeep contact rides in ADIF as MFSK plus a submode (that is what TQSL and
the services accept), but the importer read only the MODE field — so the app's own log re-read
every Tempo QSO as plain "MFSK" at launch, and the next save wrote that loss to disk
permanently. The importer now reads the identity fields the exporter writes, and a WSJT-X log's
FT4/Q65/FST4 rows (which ride the same MFSK-plus-submode shape) import as their real modes too.
A Tempo row already collapsed by an earlier build reads as MFSK on disk; the original
identity is recoverable from the one-time `log.adi.bak` made on first load.

### Fixed: logging from a named channel mis-filed the QSO's band forever

Working a contact from a suffixed band-plan channel (the 2 m FM simplex data channel, the 6 m
second channel, the DX/EU windows) stored the channel id — "2m-fm", "6m-2" — as the QSO's band,
pushed that exact string to QRZ and eQSL, earned no DXCC/VUCC/WAS credit, and could never be
confirmed. The channel id is now translated to the real band the moment it enters the app, so
the log, the uploads, the awards engine and the Needed board all see the same "2m". A QSO that
was already logged with a channel id keeps it as stored; the fix protects everything logged from
now on.

### Fixed: imported contacts claiming midnight could never confirm at LoTW or eQSL

Contacts imported without a time of day used to become 00:00:00 — and were then uploaded
asserting that midnight as fact. LoTW and eQSL match on the two operators' times agreeing, so
those contacts sat unmatched forever while the "Upload to LoTW" count never went down. The app
now remembers that a time is unknown, never writes an invented one, accepts the 4-digit HHMM
time form other loggers use, and leaves time-less contacts out of upload batches — the button's
tooltip says how many and why. Contacts an earlier build already stamped with an invented
00:00:00 are recognized too: a bare midnight with no end-of-contact time reads as "time
unknown" (a genuine 00:00 UTC contact carries one, and still counts as timed).

### Fixed: correcting a busted callsign now actually reaches LoTW and the other services

Fixing a mis-copied call used to change it only inside Nexus: the record still counted as
"already uploaded", so the correction never went out, the QSO could never confirm — and any
confirmation that had matched the WRONG call stayed attached to the corrected contact. A
callsign correction now clears the upload record (the QSO re-queues to every service under the
right call) and removes confirmations and credit earned under the wrong one. Ordinary edits —
band, grid, name — keep everything, as before. Note that LoTW itself still holds the record
uploaded under the old call; nothing an upload can send retracts it.

### Fixed: the NEW ONE badge cried wolf on entire countries, and the two DXCC counts disagreed

The awards engine identifies a DXCC entity from the callsign; the log screens compared the
free-text country name, which QRZ spells its own way — "Germany" can never match the
"Fed. Rep. of Germany" already in your log, so every German and Russian contact showed NEW ONE
forever, and Statistics and Awards counted entities differently. Every comparison now uses the
callsign-resolved entity; the country text is display only. Your log carries both spelling
families today, so you should see both numbers agree for the first time.

### Fixed: loading a saved config profile could silently remove your RF power ceiling

A profile saved before a given setting existed used to load with that setting reset to its
default — and for the per-mode power caps the default is "no cap", so loading a three-week-old
profile quietly re-armed full power at FT8's 100% duty cycle. Loading is now a merge: anything
the profile doesn't carry keeps its current value, and your callsign, license class, radio
roster and sync history never come from a profile at all. Saving a profile now snapshots the
last-saved settings rather than a half-edited form.

### Fixed: importing a master log from another logger silently threw fields away

The importer kept only the fields Nexus models and discarded the rest — contest exchanges, QSL
dates, COUNTY, and the satellite fields LoTW requires for satellite credit were gone from the
moment of import, while the manual claimed a full round-trip. Every field now survives import
and export verbatim, and the award-relevant ones (numeric DXCC entity, PROP_MODE/SAT_NAME,
OPERATOR, STATION_CALLSIGN) are first-class. Upload bookkeeping also stopped re-reading the
whole log file before every stamp, which matters on a multi-megabyte log.
### Fixed: the Phone and CW screens no longer cut off the log form — the scrollbar is back

At the default window size the Phone screen hid the entire LOG THIS QSO form below the bottom
edge with no scrollbar and no way to reach it, while the Band Activity box sat mostly empty. The
cause was a set of layout rules that promised "the view never scrolls" while a guaranteed-minimum
region and the controls below it added up to more height than any window could hold — and the
rule meant to let the page scroll had shipped in a form that could never take effect. The view
scrolls again when it must, the empty box is gone (Band Activity takes the height its content
needs), and spare room goes to the waterfall instead of to blank space. The same repair covers
CW, RTTY and SSTV, and the CW decode text now fills its pane instead of six fixed lines floating
in a tall empty panel.

### Changed: Phone and CW arrange themselves to the window — wide screens get columns

On a wide monitor the log form now takes its own column beside Band Activity and the voice keyer
instead of stretching metre-wide input fields below them; very wide screens get three columns. On
a narrow window everything stacks in a single scrolling column. Push-to-talk, the CW send bar and
Stop TX live in a fixed dock at the bottom of the screen that can never scroll away or shrink,
and switching column layouts can never interrupt a log entry you are typing or a voice-keyer
message in flight. Panes you remove in the ⊞ Panels menu behave exactly as before.

### Fixed: window sizes, splitter drags and UI-scale settings are checked before they are applied

A pane width or splitter position saved on one monitor was replayed unchecked on another: a rail
dragged wide on an ultrawide could reopen on a laptop with the centre workspace squeezed to
nothing, a UI scale pinned for a big screen carried into small pop-out windows that have no
control to undo it, and a band-map window left on a since-removed second monitor came back
entirely off-screen. Every stored size, position and scale is now validated against the window
and monitors actually present before it is used. The first paint also matches the final layout —
no more one-frame flash of wrong sizes on launch.

### Fixed: screens respond to the size they really are, and dialogs respect your UI scale

The rules that adapt layouts to narrow windows measured the raw window width, which the app's own
UI scaling makes wrong in both directions: the whole app flipped to a phone-style stack at the
minimum window size where there was actually plenty of room, and an operator who raises the UI
scale to read the screen never got the narrow layouts at all. All of it now keys on the effective
size. Satellites, DXpeditions and APRS gain the narrow layouts they always declared but that were
keyed to names that never matched anything. The setup wizard, dialogs and tooltips — which
ignored the UI scale entirely and could render at half size for exactly the operator who had
raised it — now follow it. The Logbook and radio-programming screens use more of an ultrawide
monitor instead of a fixed centre column.

### Fixed: live text panes no longer yank you back down while you read

The CW transcript re-pinned itself to the newest text every half-tick, so scrolling back to
re-read a callsign mid-copy was impossible; the SENT echo, RTTY stream, Field Day log and Tempo
conversation did the same. All now follow only while you are at the bottom, the way Band Activity
already did, and switching Tempo conversations opens at the newest message instead of wherever
the previous conversation was scrolled. Logging a contact no longer makes the screen jump.

### Fixed: opening Field Day bonuses no longer hides the Sections board

The bonus checklist grew without limit and pushed the Sections board and the newest log rows out
of a screen that could not scroll. It now scrolls within its own bounded area.

### Fixed: the call resolution details are reachable again

When a callsign resolves, the prior-contact history sits inside the log form — with the layout
repairs above, it is on screen or a short scroll away instead of clipped below the window edge at
every size, which is how it has behaved since the compact recall card arrived.
### Added: Unassisted mode — one switch for a no-assistance contest entry

Settings ▸ Contesting has a new **Unassisted entry** switch. It turns off the AI CW decoder, DX
cluster / RBN spots, and the PSK Reporter needs feed together, in one action, and takes effect the
moment you press it rather than at the next restart. Every change is written to a dated record
beside your settings, so if you are ever asked what was running during an event you have an answer
with timestamps on it.

This matters because the shipped defaults put you in an assisted category without saying so. The
AI CW decoder is on by default, and CQ WW rule VIII.2 counts "a CW decoder, DX cluster, DX
spotting Web sites … local or remote call sign and frequency decoding technology (e.g., CW Skimmer
or Reverse Beacon Network)" as QSO-finding assistance, which places an entry in Single Operator
Assisted. ARRL calls it spotting assistance and names "PSKReporter, Telnet, DX spotting websites
or bulletin board systems, automated multi-channel decoders"; Single Operator may not use it,
Single Operator Unlimited may. A footer in the CW cockpit now states which of those sources are
running, with the rule citations one click away.

Two details worth knowing. Your own settings are never rewritten: the switch overrides them while
it is on, so ending unassisted mode brings your decoder and feeds back exactly as you had them.
And your own radio's decodes keep feeding the Needed board, as do your outbound PSK Reporter
uploads, because ARRL says plainly that "Generating spotting information for use by other stations
is not considered to be spotting assistance."

Rules differ by contest and change between years. The note reports what CQ WW and ARRL currently
publish and tells you to check the contest you are entering. It does not rule on your category.

### Fixed: beacons and W1AW bulletins were offered as new countries

14.100 MHz carries the international beacon network: eighteen beacons in eighteen different
countries, each transmitting every three minutes, all day and all night. Anything watching that
frequency therefore offered 4U1UN as an all-time-new country, then the Canadian beacon, then the
Californian one, and around again forever. W1AW's code practice and bulletin frequencies did the
same thing with a station that is broadcasting one-way and will never come back to your call.

Beacons and bulletins are no longer scored as needs, no longer fire a Pounce alert, and no longer
take a new-country colour. They are still shown, because hearing a beacon is real evidence that
the band is open, and that is worth knowing. They now carry a **B** or **W** badge saying what they
are.

The suppression is by frequency, not by callsign, and that is deliberate: 4U1UN is the United
Nations headquarters station as well as a beacon site, so blanking the callsign would have hidden
a genuine new country. Heard on 14.100 it is the beacon; heard anywhere else it is the station, and
it still counts. W1AW is suppressed only on its own published bulletin frequencies, so an ordinary
contact on those frequencies, or W1AW itself operating elsewhere, still scores normally.

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

That weakest-need mix-up was not confined to the roster. The same per-station need was feeding the
map and the band strip, so a station worth chasing could be painted in the colour of the least
interesting thing about it anywhere it appeared. Every surface now takes a station's strongest
need, from one shared piece of logic rather than three.

### Fixed: the Needed board no longer claims a "new mode" you already worked

An operator with roughly 11,000 FT8 contacts was shown a new mode needed for Asiatic Russia on
30m, on a band and entity they have six confirmed FT8 contacts with. The need behind it was real —
they have never worked that entity on CW — but nothing on the screen said so, so the board looked
like it could not read the log.

Two separate things were wrong, and both are fixed.

- **The row said the wrong thing.** A mode need is judged per DXCC entity and mode class across
  *all* bands, which is what the per-mode DXCC awards count. The headline appended whichever band
  the station happened to be spotted on, turning "you have never worked Asiatic Russia on CW" into
  "New mode — CW Asiatic Russia 30m". Mode rows now read **"New mode — CW Asiatic Russia (any
  band)"** and state exactly the claim being made. Band slots remain the separate **new band** row.
- **The roster showed chips you could not act on.** Need chips on the Call Roster and Stations
  panel were keyed by callsign alone and pooled every alert for that call, so a CW mode need painted
  an unlabelled `MODE` chip onto a 30m FT8 roster. Chips, row colour, and the **Needed** filter are
  now scoped to the band and mode class actually in front of you, matching what Band Activity has
  always done. A genuinely cross-band need still shows on the Needed board, where the band is named.

Three further mis-classifications surfaced in the same audit, each of which could invent or hide a
need:

- **Phone contacts logged as `PH`** — the token the N3FJP family exports, and present in real
  imported logs — were counted as *digital*. They credited a digital mode slot they never earned
  and left the phone slot reading unworked. `PH` and the digital-voice modes (`DIGITALVOICE`,
  `DSTAR`, `FUSION`, `M17`, `FREEDV`) are now classed as phone everywhere, including in LoTW
  confirmation matching, where the mismatch could leave a confirmed contact showing as a
  confirmation opportunity forever. This is read-side: existing logs reclassify with no re-import.
- **Band labels are compared case-insensitively.** A log that carries both `30m` and `30M` (both
  spellings occur in a real logbook) credited the same slot in the awards engine but not in the
  decode feed's chip gate.
- **Nexus's own tier names are recognised on POTA/SOTA spots.** `TempoFast`/`TempoDeep`/`FT1`/`DX1`
  fell through to guessing the mode from the frequency, which on a CW-only band segment could invent
  a CW mode need.

This works together with the strongest-need ranking above rather than against it: the band and mode
gate decides which of a station's needs count on the surface you are looking at, and the ranking then
picks the most valuable of those. So a row is ranked by the best reason to work that station **that
you can actually act on right now**, and its colour, its chip, its screen-reader label and its place
in "sort by need" all name that same need.


### APRS says which radio it is listening to

## [0.21.5] — 2026-07-29

### APRS now sees the whole network, and can contribute to it

APRS used to show you exactly what your own antenna decoded, and nothing else. That is the honest
picture of what your radio can reach, but on a quiet channel it is also indistinguishable from a
broken receiver — which is what several operators were looking at.

Nexus can now also connect to **APRS-IS**, the internet side of APRS, and plot what the wider
network is reporting near you alongside what you actually hear. Turn it on in
**Settings ▸ Modes ▸ APRS**, where every APRS setting now lives, beside RTTY and CW.

- **Every station is tagged with how it reached you** — `RF` when your own receiver decoded it,
  `net` when only the internet reported it, `RF+net` when both did. You can never mistake "the
  network says this station exists" for "my antenna can hear this station", and one click hides the
  internet stations entirely, leaving the view of what this radio genuinely reaches.
- **It is also a diagnostic.** The internet feed runs whether or not the APRS decoder is armed, and
  gets its own status chip beside the decoder's. Internet stations appearing while the RF chip stays
  silent tells you the fault is in the radio chain — antenna, cable, sound card, tuning — and not in
  the app. That was previously guesswork.
- **You choose what comes through.** A radius around your grid square (150 km by default — APRS is
  a local mode), a list of watched callsigns that come through from anywhere however far away they
  are, and switches for weather stations, objects and items, and text messages.
- **No passcode needed to watch.** The feed connects read-only, which every APRS-IS server accepts
  from any licensed operator.

The internet status chip on the APRS board is also its control: click it for the feed switch, the
range radius, and your watched callsigns. The radius is there because the chip's own advice when the
feed goes quiet is "widen the radius" — the control belongs where the advice is. Both places edit
the same settings, so they can never disagree about whether the feed is on.

With the feed running you can also switch on a **receive-only iGate**: packets *your own antenna
hears* are contributed to APRS-IS, so stations around you reach the global map through your station.
It is a separate switch from the feed, and it stays in Settings rather than on the cockpit, because
it publishes under your callsign — contributing to a global network under your own call should be a
considered decision, not something a stray click can start.

Nexus only ever sends packets it actually heard on the air, and honours every rule the network asks
of an iGate: it never re-sends a packet that already came from the internet, never sends one whose
sender marked it `NOGATE` or `RFONLY`, suppresses duplicates, and caps its own upload rate so a
stuck transmitter nearby cannot flood the network in your name.

**Nexus does not gate the other way** — internet traffic is never transmitted on the air. That
direction means a radio keying up unattended, which is not something this app will do.

### The APRS map grows up

Every station on the APRS map was the same grey dot. The packets were carrying the answer the whole
time — APRS stations pick their own icon, and Nexus was throwing it away.

Stations now draw as their **actual APRS symbol**, on the map and in the station list: cars, trucks,
bicycles and people, weather stations, digipeaters and iGates, campsites, balloons, boats and
aircraft. Vehicles under way point the way they are heading. Where an operator has put an **overlay
character** on their symbol — the `I` on a full iGate, the `R` on a receive-only one, the hop count
on a digipeater — it shows on top of the icon, because that character is often the most useful thing
about the station. A symbol Nexus does not recognise draws the standard "unknown" glyph, never a
blank. The icons are drawn in Nexus rather than borrowed, so there is nothing extra to install.

Symbols also carry a colour for their family: homes and portable stations, vehicles, aircraft,
boats, weather stations, digipeaters and gateways, and hand-placed objects. Colour says what a
station *is* — nothing here means urgency. The palette varies brightness as well as hue so the
families stay apart for colourblind operators, and it has a separate version for the light theme.

**You can still tell what your own antenna heard.** That used to be the solid-versus-hollow dot.
The shape now says what a station IS, so the ring around it says how it reached you: solid for RF,
doubled when you heard it both ways, dashed and dimmed for internet-only. Solid still means yours.
Below a local scale the map goes back to plain dots — a continent covered in icons answers a
question nobody asked.

**The map opens on the local picture.** APRS is a local mode — 2 m simplex plus a digipeater or two
reaches tens of kilometres — so the map now opens reaching about 275 km in each direction, and you
can zoom in much further than before. Previously it opened at a scale where roughly 23 km fell on a
single pixel, so a station 40 km away drew less than two pixels from your own marker and an entire
local net stacked up underneath it as one dot. A freshly decoded station now appears the moment it
lands rather than waiting up to a minute for something unrelated to repaint the screen, and clicking
a station in the list highlights it on the map immediately. With no grid square set, the map centres
on the traffic you are hearing instead of painting an empty box with no coastline and no stations.

**Click a station for everything known about it.** Clicking used to highlight it and nothing else.
It now opens a detail card, from either the map or the list:

- The symbol at readable size, with what it actually **means** in words.
- **How it reached you, per source, with separate ages** — "your receiver decoded this station
  4 min ago; the internet feed reported it 20 s ago". Those are two different facts and only one of
  them says anything about your antenna, so they are never merged into a single "last heard".
- Position with grid square, and distance and bearing from your station.
- Course, speed and altitude when the station is moving.
- **The weather, when it is a weather station.** Those readings used to be shown as the raw field
  string — `220/004g011t085r000p000P000h68b10156`. Nexus now reads it: temperature, wind direction
  and speed, gusts, rainfall, humidity and barometric pressure. A sensor a station does not have is
  left out rather than shown as zero — `r...` on the wire means "no rain gauge fitted", not "no
  rain", and reporting 0.00 in would be inventing a measurement.
- The comment text, the digipeater path, and whether the packet reached you **direct or digipeated**.
- The raw packet, collapsed until you want it.
- One click to QRZ, or to the station's page on aprs.fi.

Behind all of it, the map keeps **stations**, each with its own history: last position, when it was
last heard by your radio and by the internet, symbol, course and speed. A station stays for an hour
after its last packet and starts to fade after twenty minutes of silence, so a quiet station recedes
instead of vanishing. You can change the hour in **Settings ▸ Modes ▸ APRS**, and setting "Keep
stations for" to 0 means exactly that — no fade, no removal, every station kept until the
2000-station ceiling — because some operators genuinely want an all-day picture.

### APRS tells you exactly why it is not decoding

An empty APRS screen used to mean half a dozen very different things and looked identical for all of
them: the app listening to the wrong sound card, the radio parked on another frequency, a signal
arriving too corrupted to check, or a genuinely quiet channel. Only packets that passed their
checksum ever reached the screen, so everything else vanished without trace.

The APRS header now carries a decode readout that names which one you are looking at, in six honest
states: **no input**, **silent**, **wrong frequency or mode**, **bursts heard but failing their
checksum**, **listening on a quiet channel**, and **decoding** with a count and how long ago the
last packet landed. Beside them it shows the **input level in dBFS**, so what the decoder is hearing
is a number you can read rather than something to infer from which message appeared. Hovering
explains what to check, and the empty list and empty map say the same thing rather than a generic
"nothing here".

**A closed squelch is not a broken audio device.** A squelched radio does not send the app silence
in the sense of *nothing*; its USB codec keeps streaming a continuous run of digital zeros. Audio is
arriving the whole time — it just has no level. So **"Silent"** (the input alive with nothing on it,
almost always the squelch closed between packets) is a separate state from **"No input"** (no audio
samples arriving at all), and only the second is a fault. An idle FM channel between packets is what
APRS looks like nearly all the time, so "Silent" is not coloured as a problem; it says to open the
squelch and watch for hiss if you want to confirm the routing. "No input" really does mean the
capture device is wrong or gone, and still points you at Settings.

**A mistuned radio is named as one.** FT8 decoding beautifully on 2 m at the same moment the APRS
screen insists there is no audio are both true statements: the radio has one receiver and one dial,
and parked on the FT8 frequency in USB it is never receiving the APRS channel at all — so every
message about audio levels would be advice about the wrong problem. The readout now looks at the
radio itself, says so first, and offers a one-click fix: **"The radio is on 144.174 USB — APRS needs
144.390 FM"**, with a Tune button beside it. It judges against the APRS channel *you have selected*,
so 144.800 in Europe or 145.175 in Australia is correct, not a warning. Sitting on the right
frequency in the wrong mode is its own trap — the signal looks strong and decodes nothing — so that
case reads **"on 144.390 but in USB — APRS needs FM"** and explains that FM packet audio demodulated
as SSB is garbled. Data-FM submodes such as PKTFM count as FM, because on the air they are. Tuning
while an FT8 over is in flight cannot move the radio immediately — the rig will not accept a
frequency change mid-transmission — so rather than appearing to do nothing, the Tune button says the
radio will move when the over ends.

**It says which radio it is listening to.** If more than one of your radios covers the APRS band,
the readout names the one it is actually listening to — "on FT-991A" — and its tooltip explains that
APRS follows the active radio and that routing rules decide which radio a band goes to. Without
that, a station whose APRS audio is set up on one rig while the app listens to the other has exactly
one symptom: silence. A working station looks like a dead band. With the radio named, that is a
glance instead of an afternoon. On a single-radio station, or when only one radio covers the band,
nothing is shown — there was no choice to make and saying so would just be clutter.

**Every claim says when it was true.** The packet counts run from the moment you arm the decoder,
while the level is whatever the radio is doing this instant, and mixing the two produces sentences
that contradict themselves: *"2 packets were heard but none passed the checksum... peak -99 dBFS."*
Nothing is heard at -99 dBFS. A failed-checksum count now only speaks in the present tense while
bursts are still arriving, within the last minute, and dates itself when it does: *"2 bursts heard
since arming, last one 20s ago — none passed the checksum"*, with the live level on its own clause.
Decodes are treated differently on purpose — a packet that passed its checksum proves the whole
chain works, and that stays worth knowing however long ago it was, so it keeps its place and carries
its age instead: *"18 packets decoded since arming, last one 12m ago."* The level reading says what
window it measures, the most recent tenth of a second, so a low number reads as the gap between
packets rather than something being wrong. And once packets are decoding, the readout stays on the
decode count instead of flicking back to a warning during the quiet gaps between them.

Three smaller pieces of the same honesty. A failed-checksum count explains that a packet caught
part-way through — which is what happens when the squelch opens mid-burst — can never pass its
checksum, so some failures on a busy channel are expected rather than a sign of a misconfigured
radio. Packet-shaped patterns found in silence do not count as packets at all: given enough minutes
the decoder will eventually find one in the noise floor, and reporting that as "packets heard"
invents evidence for a problem that is not there. And the Monitor button, the decode readout and the
empty-state text all report the decoder's actual state rather than the button's own guess, so
leaving the APRS screen and coming back can never show "Monitor" — as though nothing were running —
while packets keep decoding into the list beside it.

None of this is covering for a fragile decoder. The packet decoder was measured against "twist" —
the two packet tones arriving at unequal volume, which is the classic reason packet decoders
struggle on real signals — and packets still decode with the tones up to 24 dB apart, far beyond the
roughly 9 dB that real signals show.

### APRS starts listening when you open it — receive only

Opening APRS now starts the decoder for you, so the screen is not dead until you find the Monitor
button. This is strictly receive: a decoder started this way will **never** send an automatic ack,
whatever your TX setting.

Automatic acks stay behind two deliberate acts, and opening a screen is not one of them: you arm
Monitor yourself, **and** TX is on. That is now enforced rather than assumed — an unattended
transmission should never follow from navigating somewhere. The Monitor button says which state you
are in, reading "Monitoring (auto)" when APRS started it for you, and its tooltip spells out whether
acks can go out.

Clicking Monitor always means start or stop, as before. It never quietly upgrades an
automatically-started decoder into one that can transmit — to allow acks, stop it and start it
yourself. And if you stop the decoder, it stays stopped: coming back to the APRS screen will not
restart it behind you.

### Route each mode to the radio that does it best

Nexus already handed a band to the radio configured for it: pick 2 m and it switched to your VHF
rig. But a band is not fine enough. If you have a 2 m/70 cm rig for weak-signal digital and a
different rig for FM and APRS, both of them cover 2 m — and Nexus had no way to tell them apart, so
a 2 m FT8 spot and an APRS tune went to whichever radio it happened to pick first.

You can now route on the band **and the mode**. In **Settings ▸ Radio** there is a routing table
under your radios: pick a set of bands, pick a mode class, pick the radio. Rules are checked top to
bottom and the first match wins, so a specific rule above a broad one takes precedence — and the
arrows beside each rule let you reorder them. Anything no rule matches falls back to the band
coverage you already set on each radio, and then to a default radio you can nominate for everything
else.

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

**And a third radio now works properly.** Two radios worked. A third did not, for a reason that
only ever shows up at three: each radio's window keeps its own settings file, seeded once from the
shared one the first time that window opens. With two radios you always add the second one before
those per-window files exist, so both windows learn about both radios. The third radio is the first
one you add *after* they exist — so it landed in exactly one window's settings and nowhere else. The
launch picker (which reads the shared file) never offered it, the other window never monitored it,
and there was no way to repair it from inside the app.

Adding or removing a radio now updates the shared config too, and every window picks up radios added
elsewhere when it starts. The routing table above is shared the same way, since which rig does 2 m
FM is a decision about your station, not about one window.

Three smaller things that also only bite at three radios: a band claimed by two rigs now always goes
to the same one (it used to depend on the order they happened to sit in the list); adding a radio
after removing one no longer produces two radios with the same name, which made the port and audio
conflict warnings ambiguous; and a window launched pointing at a radio that no longer exists now
says so instead of quietly driving the first radio's serial port — which is the port another window
is already using.

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

Program's per-repeater Tune button tunes in a single step that knows it is FM, which is what makes
it land correctly on a multi-radio station and after you have been operating something other than
voice. Naming FM explicitly settles both decisions at once: the machine's frequency, shift, offset
and tone all go to the radio you mapped for FM, and the rig ends up in FM rather than in whatever
data mode the last section you operated left it in — a repeater is inaudible in a data mode. Tuning
does not move you out of Program or arm transmit; it puts the radio on the repeater so you can
listen. Any later retune, section change, radio switch, or a turn of the VFO knob down to HF
releases the FM hold, so FM never follows you somewhere it does not belong.

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

**Clicking an operation also opens its webpage** in your browser, so the announcement you are
looking at is one click from the team's own page — bands, schedule, QSL route, pilot station. The
Details rail carries the same link on each entry, labelled, so you can see where it goes before you
click it. About a third of announced operations publish a website, and the calendar source has been
carrying those links all along — Nexus was throwing them away while reading the page. The rest now
open the callsign's QRZ page instead, which is where their details and QSL route live when there is
no expedition site. Either way the tooltip names the destination first, and says plainly when it is
the QRZ fallback rather than the operation's own page. Clicking a calendar bar still selects that
operation in the Details rail as it did before, so nothing that used to work costs you an extra
click now.

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
on with the address field empty and Nexus fills in the usual local target for you. The address
field now also states which of the two it is doing, so a configured-but-silent output can never
look like a working one again.

It is off after an upgrade, and nothing but that switch can turn it on — your contacts do not
start going out over the network because you installed a new version.

Field Day is untouched. During an event, contest contacts still go out the way they always have,
carrying your class, section and points; the standing broadcast only ever carries the contacts in
your regular log. A contact is never sent twice, so it is safe to leave the switch on through a
Field Day weekend. An ordinary QSO carries what a map needs — call, grid, band, frequency, mode,
time — and honestly claims no contest points.

If you run several consumers on one machine, name the port. 12060 is often already taken (HRD
listens there), and the port you type is the port that is used.

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
not accept that frequency, and stops asking after a few tries instead of hammering the link. A
command your rig will not take no longer wedges rig control until you restart the app, whatever the
rig and whatever the command.

**Rig control recovers on its own.** Nexus stops polling a radio that has stopped answering, which
is right — but that state used to be permanent, so any hiccup meant no rig control until you
restarted. It now retries quietly, backing off to about once every thirty seconds, and picks the
radio back up within a couple of seconds of it answering again. This one is not specific to APRS:
anything that interrupted the link used to cost you rig control for the rest of the session, on
every rig Nexus talks to.

**In the cockpit**, an HF-only station now reads *"No 2 m radio"* with an explanation, instead of a
Tune button that could only ever fail. The internet feed is genuinely useful without a VHF radio —
it shows APRS traffic other stations have reported — so the view tells you that rather than
looking broken.

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
