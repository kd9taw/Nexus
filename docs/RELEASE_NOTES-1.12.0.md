Nexus 1.12.0 takes contesting past Field Day: Sweepstakes, CQ WW, CQ WPX, ARRL VHF and four state
QSO parties. JS8 is now built in. Long overs no longer go silent partway and leave the rig keyed.
Connect lets you close the panes you don't use, and runs far lighter on the CPU. And HRDLog.net
uploads work for the first time.

The 1.11 releases only ever went out as betas. If you're coming from 1.10.3, everything below is
new to you, including JS8, the credential fixes and the illustrated manual.

## Worth reading even if nothing looked wrong

- **A long over could go silent and leave your rig keyed.** WSPR stopped sending audio about twenty
  seconds in, and the radio stayed keyed for the rest of the two-minute over, every over. JT65,
  Q65 and FST4/FST4W at 30 seconds and longer did the same. FST4W-1800 held the transmitter up for
  twenty-nine minutes per over. A one-shot PSK31 send longer than about two lines went out cut
  short, with the rig keyed for the whole message. On SSB that meant a keyed radio putting out
  nothing. On FM, AM or CW it would have been a dead carrier. Fixed: the whole transmission now
  goes out, and the rig unkeys when the audio really ends.
- **A rejected QRZ Logbook or ClubLog upload could write your API key into `log.adi`.** When
  either service turns an upload down, its reply quotes your request back, key included. Nexus
  was storing that reply on the contact, so the key rode out with every export built on your log.
  That included the batch TQSL signs with your certificate and sends to LoTW, eQSL uploads, and
  any export you made. Nexus now records only its own description of why the upload failed. On
  upgrade it cleans the old replies out of `log.adi`, out of `log.adi.bak`, and out of the dated
  copies in the `backups` folder. The contacts themselves are kept, and so is the fact that the
  upload bounced. Only a rejected upload ever wrote anything.
- **`settings.json` could be read by any account on the computer.** It holds your ClubLog key.
  It's now owner-only. An existing file is tightened the next time Nexus saves it.

There's nothing to do for any of these but upgrade. More on credentials under
[Your credentials and privacy](#your-credentials-and-privacy).

## Contesting

Pick a contest in Settings ▸ Contesting and the whole operating path follows it. The entry strip
shows that contest's exchange. Dupe checking uses that sponsor's own rule. The scoreboard counts
its multipliers, and the Cabrillo and ADIF exports carry its names. Every rule was read from the
sponsor's current rules page. Where a sponsor's own wording is unclear, Nexus notes the ambiguity
rather than guessing.

- **ARRL November Sweepstakes, CW and Phone.** Each weekend is its own entry. The long exchange
  is handled: serial, precedence, callsign, check and section. Change your check partway through
  the contest and Nexus tells you when you log, and it won't export a log with two different
  checks in it. The refusal names both values.
- **CQ World-Wide DX and CQ WPX, CW and SSB.** Their multipliers come from the other station's
  callsign (country, CQ zone and WPX prefix), so Nexus works out each contact's country and zone
  as you log it. These contests need a country file. Without one, Nexus won't start the contest,
  rather than quietly score your weekend at zero.
- **ARRL VHF: January, June and September.** They're separate picks because they score
  differently: January pays more for 902 MHz and up. Each has its own weekend and its own
  Cabrillo and ADIF names.
- **Four state QSO parties: California, Ohio, Tennessee and Texas.** Ohio has three roles
  (in-state, W/VE and DX). The one you send follows your station data, so you don't type it
  each time. Tennessee and Texas have bonus points that aren't in the on-screen total, and the
  scoreboard says so. The log is still good to submit. The number on screen just isn't your final
  claimed score.
- **A rate meter on the scoreboard.** It shows contacts per hour over your last 10 contacts,
  your last 100, and the last 60 minutes. It works for every contest, both Field Days included.
  It drops when you stop, instead of showing your best run on a dead band.
- **A button to merge the contest into your logbook.** It tells you what it will do before you
  press it ("Merge 3 contacts into my logbook"). Afterwards it says how many were added and how
  many were already there, so a second press is plainly harmless. It sits next to the switch that
  decides whether merged contacts get queued for upload. One catch is written underneath: ClubLog's
  own catch-up still sends any contact ClubLog never accepted, the next time you save a ClubLog
  password, whatever that switch says.
- **Single-op or multi-op.** A club running multi-op can declare it under Settings ▸ Contesting.
  Single-op is the default. The Field Day Cabrillo header used to say MULTI-OP on every log Nexus
  exported, so every solo entry claimed to be multi-op. It now follows your entry category. A club
  host's merged export still says MULTI-OP, because that one is.
- **Your ARRL section list is current: 85 sections.** The old list was eight years out of date.
  `GTA` is renamed `GH` and `NT` is renamed `TER` for you. `MAR` split into `NB`, `NS` and `PE`,
  and only you know which one you're in, so Nexus keeps what you had, tells you what changed, and
  asks.
- **Field Day won't start on a section ARRL doesn't list.** It used to start anyway and let you
  send it. If you operate from a section Nexus doesn't know, it will now stop you where it didn't
  before.
- **Three Cabrillo contest names were wrong.** Field Day, Ohio and Texas now go out as `ARRL-FD`,
  `MRRC-OHQP` and `TXQP`. The ADIF contest IDs were right all along.
- **N1MM broadcasts named the wrong contest.** Anything that wasn't Winter Field Day was announced
  as ARRL Field Day.
- **The WSJT-X and N1MM feeds sent the wrong exchange for earlier contacts.** They stamped your
  current exchange on every contact, so a mobile station that changed county mislabelled everything
  it had already worked. Each contact now carries the exchange it was actually sent with. Field Day
  operators see no change.
- **The RTTY sequencer's sign-off sent the next contact's serial number.** A serial is now issued
  once, when you first send the exchange to a station, and every repeat to that station carries the
  same number. An abandoned contact's number is skipped, not reused.
- **The RTTY sequencer couldn't log a Winter Field Day contact.** It was matching the ARRL Field
  Day classes A–F, so a legal `2M EPA` never completed. It now copies against the classes of the
  event you picked.

If a score or an export looks wrong for a contest you know well, that's exactly the report I want.

## JS8

- **JS8 is built in, and it's on.** Nexus decodes and transmits JS8 itself, with no second program
  and no audio routing. It's in the Digital group of the sidebar, after APRS. Open it and Nexus
  tunes the JS8 watering hole for your band and decodes all four speeds at once. Nothing transmits
  until you enable TX in the header and switch on what you want sent. If you don't run JS8, turn it
  off in Settings ▸ Appearance ▸ Features.
- **It transmits like JS8Call.** Your messages, CQ, and heartbeats on a random free slot between
  500 and 1000 Hz. Autoreplies, relay and HB-ack each sit behind the TX latch plus their own
  switch, and each shows a countdown you can cancel. Stop TX clears the queue, the heartbeat
  schedule and any pending reply.
- **CQ and the heartbeat repeat on their own**, with the countdown on the button: `CQ (12)`,
  `HB (42)`. Set the CQ interval in Settings ▸ Digital ▸ JS8 ▸ CQ repeat interval. At 0, CQ stays a
  single shot. It's the POTA and beacon habit: set it going and walk away. It sends nothing unless
  TX is enabled and the repeat is switched on. A station answering you turns the CQ repeat off, as
  in JS8Call. The 60-minute idle watchdog stops both, and a scheduled call doesn't reset it. Stop
  TX, leaving JS8 or changing mode cancels the schedule, and it's never remembered across launches.
  Nexus can't start up calling CQ.
- **The station list carries what a DX operator needs:** distance, beam heading, a worked-before
  tick (any band, any mode, as JS8Call counts it), the name and comment from your last contact, and
  a pin for stations you're watching. A second pane lists band activity by audio offset with its
  time delta, so finding a clear slot is a glance. Both panes can be hidden from ⊞.
- **Where it stands.** The four speeds haven't been run side by side with JS8Call on the air yet.
  Fast and Turbo have only been decoded from generated audio, never off a real band. At Normal,
  Nexus needs roughly 2 dB more signal than JS8Call to print the same message. Reports from the air
  are welcome.

## FT8, FT4 and other digital modes

- **Hound is one click on the FT8 screen.** It used to be a dropdown backed by a setting. Hound
  is a per-DXpedition mode, so it still turns off at every restart. Toggling it mid-QSO is now
  safe: a contact keeps the rules it started under. Before, switching Hound off during a Fox
  contact could leave you calling a station that had already rogered you. SuperHound is no longer
  offered, because it never did anything plain Hound didn't.
- **Nexus warns you when a DXpedition is running SuperFox.** Nexus can't decode SuperFox, so
  those operations never show in the decode list, and mid-pileup that looks just like bad
  propagation. When the DXpedition calendar shows one on the air, the FT8 header and its card on
  the DXpeditions board say so: work that one in WSJT-X.
- **A POTA activity map for park hunting.** A new Map button beside Classic/Roster opens the map in
  its own window, so you can put it on a second monitor. It shows every spotted POTA activator,
  coloured new or worked, with park, band, mode and spot age on hover. Double-click a park to QSY,
  set the mode and tag it as your hunt target. It never keys the transmitter.
- **An alert for new POTA activations.** Settings ▸ Spots & Alerts ▸ New POTA activation, off by
  default. Nexus beeps once when a park comes on the air, never for the ones already active when
  you turned it on. The map doesn't need to be open.
- **CW and SSB park activators show for digital-only stations.** If you'd set Nexus up for FT modes
  only, a CW or SSB activator was dropped from the roster, so you saw fewer parks than GridTracker
  did. A park is someone to hunt whatever the mode, so they now stay listed.
- **F4 clears the callsign card too, and works while you type.** F4 used to clear only the DX Call
  and Grid boxes, and nothing cleared the card. It was also dead while the cursor sat in a text
  box. Alt+F4 still just closes the window.
- **The 60 m FT8 dial.** The band button tunes 5.3715, and that's right for US stations: after the
  FCC's February 2026 change, US FT8 moved there to keep 100 W. Outside the US, or running QRP,
  5.357 is still the one you want. It's a Memories preset. A 60 m spot on either dial now shows as
  FT8, and the 60 m notes on the Memories presets are corrected.
- **Hold FM-D while receiving SSTV.** This is a per-radio switch under Settings ▸ Radio ▸ Rig & CAT
  ▸ Advanced, off by default. It's for sitting on an FM SSTV calling channel for the evening. With
  it on, the radio stays in the data submode as long as the SSTV receiver runs, even after you
  leave the SSTV screen. **Stop the receiver before you go back to voice**, or your microphone
  modulates nothing. ⚠️ Not yet checked on a real IC-9700 held in FM-D between pictures.

## Transmitting safely

- **Long overs go out whole.** This is the fix from the top of these notes. It was reported on an
  FTdx10 under Ubuntu. Nexus handed the sound card the whole over at once, and the card only held
  about twenty seconds of it. The audio now feeds the card as it plays. FT8, FT4, FT2, MSK144, the
  15-second Q65 and FST4 periods, SSTV, CW, RTTY, the tune carrier and continuous PSK31 were never
  affected.
- **Your transmit timing no longer leans on one network reply.** Nexus corrects for your PC clock
  being off. That correction used to come from whichever time server answered first, with nothing
  to check it. Now it asks three servers, needs two to agree within 200 ms, and confirms with a
  second round a few seconds later. If they don't agree, nothing changes.
- **The clock correction holds when you lose internet.** It used to vanish on the first missed
  check, which hit portable stations hardest. The last good correction is now kept, with its age,
  and the top bar tells you when it's too old to trust.
- **A clock more than a minute out isn't corrected silently.** Nexus won't steer by an offset that
  big and tells you instead. Set your computer's clock: your logged QSO times come from it too.
- **Waking a laptop from sleep no longer applies a stale correction.** Nexus spots the clock jump,
  drops the old correction and re-measures straight away. On Windows it also asks the time service
  to re-check. If the jump lands mid-transmission, the over is ended rather than left keyed.
- **The clock chip in the top bar says what Nexus is doing.** It used to show a red alarm on
  stations whose timing was fine. It now tells you whether Nexus is applying a correction (and how
  old it is and how many servers agreed), whether the correction has aged out, whether the clock is
  too far off to correct, or whether you need to set it yourself. If you run NetTime, Meinberg or
  Dimension 4, it says Nexus is leaving your clock to that program.
- **On Windows, Nexus fixes a broken time service.** Maybe a "debloat" script switched it off, or
  it never synced, or it only checks every nine hours. Nexus fixes that with one administrator
  prompt, once, and only on a machine a check has already found broken. A healthy machine, or one
  running its own time program, is left completely alone. Nexus never changes which time server
  your computer uses. On Linux and macOS nothing is changed.
- **Nexus's own time check no longer uses the NTP Pool.** It now asks operator-run public time
  services, and spreads out its check times so a thousand stations don't all ask at the same
  second.

## Connect and the map

- **Close any pane you don't use.** Every pane on Connect has a ✕. The Panels menu brings back
  anything you closed, with Undo and Reset layout. When a pane closes, its neighbour takes the
  room, and if you close a whole side, the map takes the width.
- **Make the map bigger.** Drag the edge of a side column, or the line between its two panes. That
  works from the keyboard too, and a double-click puts the default back. Layouts are remembered
  per window and trimmed to fit a smaller screen. Nothing changes until you use it, and Reset
  layout puts Connect back the way it first opened.
- **One map picker: Globe · 3D · Flat · Beam.** The separate 2D/3D switch is gone. Leaving the 3-D
  globe used to land you on a 2-D globe that looked the same, so the switch seemed to do nothing.
  The four sit in one row in the same place on every map. A new view starts on Globe, and each view
  remembers the one you picked.
- **Each view keeps its own map.** Chase DX, POTA/SOTA and the others each remember their own
  projection, layers and colours, so a trip to POTA/SOTA no longer resets your Chase DX map. A
  view's preset only applies the first time you open it, and whatever you had set before upgrading
  is kept for the view you were on.
- **Layers sits top-left on every map** and folds away the same way Conditions does.
- **Full-screen map.** One button on the map toolbar gives the map the whole window. The same button
  or Escape brings everything back. A popped-out map remembers this separately from the main
  window.
- **The map draws your paths.** Green dashed great circles run to the stations that reported hearing
  you, and these are on by default. Blue dotted ones run to the stations you decoded. They're off by
  default, because on a busy FT8 band that's a hundred lines; one checkbox in Layers turns them on.
  Each set has its own opacity slider and shows at most 30 paths, freshest first. Paths drop off
  after 30 minutes, or when the station goes stale. A station heard both ways draws once, in green.
  The 3-D globe's heard-me arcs are now capped the same way.
- **Much lighter on your CPU.** The 3-D globe used to redraw constantly with nothing moving. The
  2-D map redrew every country outline each time new data arrived, even when nothing on it had
  changed. Both now redraw only when something changes, and with every layer off they sit near
  idle. If Connect made your laptop fan spin up, try it again.
- **Everything on the map toolbar fits at 1024×768.** Reset, Full screen and the LIVE badge used to
  be cut off on a small window, and the Layers panel could open off screen. The toolbar now wraps
  onto a second row.
- **Map markers are sharp again at UI scales above 100%.** Station and spot markers also scale with
  the map and carry a dark outline, so they stay readable over coastlines. The hover card now stays
  on the marker instead of following the cursor.
- **The Chase pane keeps updating after you select a station.** Its "open now / best" column used to
  freeze once you picked a station. It now refreshes every minute.
- **A popped-out POTA/SOTA board scrolls.** Anything below the fold was unreachable.

## Logbook and uploads

- **HRDLog.net uploads work.** They never had, on any computer. HRDLog's server only offers
  encryption that Nexus's built-in secure-connection library can't use, so the connection was
  dropped before anything was sent. Nexus now uses your operating system's own secure connection
  for HRDLog.net, and only for HRDLog.net. Every other service is unchanged. The certificate is
  still checked, and your upload code stays in the system keychain, out of every error and the
  connection log. If that connection drops while it's being set up, Nexus now says so, instead of
  blaming your antivirus or a proxy. A failed upload is retried twice, as HRDLog asks of logging
  programs, instead of up to twenty times. On Linux, Nexus now needs OpenSSL 3 (`libssl3`): the
  .deb installs it for you, and the AppImage carries its own copy.
- **Export one POTA activation.** The Logbook's export area lists your activations, each with park,
  UTC date and contact count, and exports exactly the one you pick. Ordinary contacts from the same
  day no longer end up in the file. It's named the way POTA asks, like `KD9TAW@US-1234-20260909.adi`.
  Two parks in a day gives you two files. An activation that crosses UTC midnight also shows as two,
  because POTA credits each UTC day separately. Six contacts either side of midnight reads "6 QSOs ·
  under 10" twice, so you see it before you upload. Hunter contacts never land in an activator
  file, and picking an activation overrides any leftover date range. Not handled yet: a park that
  spans two states (the filename needs a part Nexus can't fill in), and two-fers.
- **Correcting a busted callsign re-sends the contact.** Fixing a call in the Logbook's edit form
  used to leave QRZ, ClubLog, eQSL and the rest holding the wrong call for good. A callsign
  correction now goes back out to every service you upload to. Other edits (name, grid, RST, park)
  still don't re-upload.
- **The edit form's CALL box shows the whole callsign.** In a normal window it was about five
  characters wide, so a six-character call looked cut off. What you saved was always right.
- **AM contacts log as AM.** Work someone on AM from the Phone screen and the log used to say SSB,
  even on 14.286. The log now follows the mode your radio is actually on, as long as CAT is up and
  the radio has answered. So an AM contact logs as AM, and if your rig didn't take the AM you picked,
  it logs as the SSB it really was. The entry strip shows the mode before you log, and ADIF, N1MM
  and N3FJP feeds and your uploads all carry it. This fixes new contacts only. An AM QSO already
  logged as SSB is yours to correct in the Logbook.
- **AM is on the Phone screen's mode picker on every band.** It was hidden below 10 MHz and from
  28 MHz up, which left no AM button on 14.286. A radio that can't do AM somewhere still refuses it.
- **Your log records which callsign made each contact.** Work a weekend as a special-event call,
  put your own call back, then upload, and the whole event used to reach LoTW under your home call.
  Every contact now carries the call that made it, in the log and every ADIF export. LoTW and HRD
  Logbook uploads sign from that. Contacts from earlier versions upload as they did before.
- **Upload failures say why.** Cloudlog and Wavelog now show the instance's own reason: a read-only
  key, an unlinked station profile, a missing field. Can't reach the instance at all? Nexus names an
  antivirus or company proxy as the likely cause, not your URL. The Cloudlog row keeps which failure
  it was after a restart. A LoTW upload TQSL turns down now tells a missing or expired Callsign
  Certificate apart from a bad Station Location, and TQSL's own message is in the connection log. A
  LoTW report that fails to download says what went wrong, and suggests a narrower date range if it
  keeps happening.
- **The QRZ callbook row only goes green when a lookup proves your subscription.** When an XML
  subscription lapses, QRZ still answers lookups, just without grid or state, and the row stayed
  green. It now needs a subscriber-only field in the reply, and it goes red if QRZ says you're not
  a subscriber. The trade-off: looking up a call that doesn't exist, or one with no grid or state
  on file, reads as failing until your next good lookup. The Test connection button on the QRZ
  Logbook row now updates its dot too.
- **A pasted upload code with a stray space or newline no longer fails every upload.** HRDLog.net
  codes, QRZ Logbook keys and World Radio League keys are trimmed when you save them. A code
  already stored is fixed when you re-enter it.
- **A connector row no longer reads green in the second it failed**, and a manual HRDLog.net or
  World Radio League upload that never got through now shows on its row.

## Your credentials and privacy

- **Your API key no longer ends up in `log.adi`**, and upgrading cleans out the old copies. See
  [Worth reading](#worth-reading-even-if-nothing-looked-wrong) at the top.
- **A service's error text is no longer saved in your config folder.** Cloudlog and Wavelog replies
  can carry your API key, and the Connections panel was saving them word for word. HRDLog.net and
  World Radio League replies were saved too, with no length limit. Nexus now saves only its own
  sentence. You still see the service's exact words in the upload result and in this session's
  connection log. A saved line from an earlier version is dropped the next time Nexus starts.
- **On Linux, service replies no longer land in your desktop session's log file.** That file is
  readable by any account on the machine, and Nexus was echoing service replies into it, keys
  included. That's fixed for the connection log, the ClubLog most-wanted fetch and the hourly QRZ
  Logbook sync.
- **`settings.json` is owner-only.** See the top of these notes.
- **Upgrading no longer deletes a stored Cloudlog key when the keychain isn't available.** On a
  Linux box with no Secret Service running, the key was silently lost at launch. It's now removed
  from the file only once it's safely in the keychain, and Nexus tries again on a later launch.

## Radios, CAT and amplifiers

- **Nexus no longer puts your radio in CW-R on 40, 80 and 160 m.** It was applying the 40/80/160 m
  LSB convention to CW, where it doesn't exist, and on an Icom it even landed on the wrong side. Nexus
  now asks for plain CW everywhere, so your radio uses whichever CW its own menu sets. If you want
  reverse CW, tick **Reverse CW (CW-R)** under Settings ▸ CW. It's off by default. The top bar now
  flags CW against CW-R as a mismatch. FlexRadio SmartSDR, PowerSDR and Thetis have no reverse CW,
  so turning it on there leaves that warning showing. The soundcard keyer is unchanged. Thanks to
  the operator who spotted it on an IC-7300.
- **Xiegu radios: trust the radio's own SWR meter.** Nexus reads the meter on an Icom scale, and a
  G90 showing 1.2:1 on its front panel can read 6:1 in Nexus. The Xiegu rig guide says so now. A
  proper calibration needs bench measurements first.
- **Amplifier band-following does what the checkbox says.** The checkbox and your radio profile
  could disagree after a save, a restart or a radio switch. They now stay in step, and your saved
  choice is kept. Nexus also re-checks your setup before every command it sends the amp. Turning
  follow off or changing the port cancels a pending band step, and a button you press wins over an
  automatic one. Nothing is sent while you have the radio keyed by hand. The KPA waits for a fresh
  idle reading, and SPE amps stay within their model's band limit. ⚠️ Not yet verified with a real
  amplifier.
- **A radio that won't connect backs off.** A background radio that fails to connect waits longer
  between retries, up to a minute. Fixing its settings retries straight away. Switching to a radio
  that's still connecting no longer opens a second connection on the same port.
- **Windows sound card glitches no longer flood the log.** One operator's log was mostly "capture
  stream died" while nothing had died. That's Windows reporting one late packet. The message is now
  rate-limited and no longer calls it a death. Receive audio also gets about 100 ms more buffer,
  with no added delay. The "no decodes" log line now says whether any audio is reaching the decoder,
  which tells a dead band apart from a dead audio path. ⚠️ Not yet measured on a Windows machine.

## The app

- **Start Nexus when you sign in.** A new switch under Settings ▸ Appearance ▸ Start at sign-in, off
  by default. It's an ordinary start: Nexus listens, and transmit stays off. If your computer won't
  let Nexus start at sign-in, Nexus says so and the switch stays off.
- **The beta-updates switch no longer turns itself off.** Touching an APRS control after turning it
  on wrote an old copy of your settings over it, so you'd quietly stop getting betas. Fixed at the
  source, so no other setting can be reverted that way.
- **The diagnostic log's "Always on" says why.** A log you can switch off is missing on the launch
  that needed it. The one switch that is yours is Settings ▸ Logging & Connectors ▸ "Extra detail
  in the diagnostic log", and it applies without a restart. There is no `--debug` flag; `--profile`
  is the only argument Nexus takes.

## The manual

- **The manual has pictures now.** All 22 chapters are illustrated, with 137 screenshots, up from 11.
  The first-run wizard, the waterfall and the settings reference got the most attention.
- **A stack of out-of-date claims were corrected.** The wizard is four steps, not three. The
  waterfall takes three mouse gestures, not two. The grid wants all six characters. Several mode
  chapters described transmitting those modes can't do. Where a picture would have had to be
  staged, there's no picture, and the text says what's missing.

## Also fixed

- **The satellite catalogue no longer serves years-old orbits.** Birds that dropped out of tracking
  kept their last recorded orbit, the oldest from 2014, and four had already re-entered. Orbits more
  than 30 days old are no longer published. Those birds keep their row with a status chip saying
  why. ISS, SO-50, RS-44, AO-91, AO-7, QO-100, PO-101, AO-73 and FO-29 were never affected.

---

Nexus Remote, operating your station from a browser, is in the app as an early pilot. More on that
soon.

73 — KD9TAW
