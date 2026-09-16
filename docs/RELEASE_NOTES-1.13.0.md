# Nexus 1.13.0

The headline is that the browser became a radio you can operate, not just watch — and that a
month's worth of shack-side fixes finally met a radio. **83 changes since 1.12.0.**

If you run Nexus Remote, this is the release where it grew up. If you don't, the things most
likely to matter to you are further down: phone contacts now record which sideband you worked,
two new SSTV mode families decode, the band dropdown shows how each band is doing, and a handful
of transmit-path checks that were judging the wrong frequency now judge the right one.

## The short version

- **Nexus Remote operates the station.** Split, XIT, RIT and VFO from a browser; clickable FT8,
  FT4, CW, Phone and RTTY spots; the logbook entry form, edits, deletes and QSL marks; start and
  end an activation; spot yourself to pota.app; work a satellite pass; hear the receiver.
- **Phone contacts record their sideband.** Every phone QSO used to log as plain `SSB`. The mode
  stays `SSB` and the sideband rides as the ADIF `SUBMODE`, which is what LoTW accepts — and the
  sideband comes from your radio over CAT or not at all, because a guessed one in a permanent
  record is worse than none.
- **Two new SSTV mode families**: Wraase SC-2 180 and Pasokon P5 and P7. Pictures paint as they
  arrive, a received picture opens full size in its own window, and you can start a receive by
  hand without waiting for a header.
- **The band dropdown shows how each band is doing** — a coloured dot and a word, from the same
  source as the band-conditions strip, in every cockpit.
- **Transmit-path checks judge the right frequency.** XIT is counted in the licence check, an FM
  repeater is checked at the machine's *input*, and XIT and VFO changes are held back while the
  radio is transmitting.
- **Two Nexus builds on one computer no longer erase each other's settings.** If you run a tester
  build alongside the release, the older one used to silently write the newer one's settings out
  of existence.
- **A new setting stops transmitting when SWR is high** — off by default, threshold adjustable,
  and only on radios whose SWR scale Nexus can trust.

## Two notes on what was verified, and what was not

Everything in this release ran on real radios before it shipped: twelve CAT and transmit rows on
the bench, two radios operating simultaneously, the FT transmit sign-offs, and — for the first
time — **JS8 on all four speeds against a real JS8Call station**. JS8 has been in the app since
1.12.0 with Fast and Turbo verified only against synthetic audio; that gap is now closed.

Not in this release, deliberately: a fix for two Nexus instances sharing one rigctld and both
commanding the same transmitter. The supported way to run two radios — the radio picker, one
profile per instance — is confirmed working. The fix for the other path is written and waits for
its own bench.

---

## [1.13.0] — 2026-09-16

### Added

- **Nexus Remote grew from watching the station to operating it.** Over this release the hosted
  browser stopped being a window onto the shack and became a way to work it. The individual pieces
  are listed separately below where they stand on their own; this is the shape of the whole.

  **Operating.** Split, XIT, RIT and the VFO choice are on the hosted page. Spots on the Spots,
  Needed, map and DXpedition boards are clickable and now include FT8 and FT4 as well as CW and
  Phone. The AI CW decoder can be switched on and off and an FT8 or FT4 period re-decoded. The rig's
  scope span, reference and position can be set. An FM repeater can be tuned from the hosted Program
  list — licence-checked at the machine's *input*, exactly as at the shack — the APRS channel set
  from the APRS board, and a stored memory recalled from Memories. **Every one of these needs station
  control, and every one is still refused while the rig is keyed.**

  **Logging.** The hosted Logbook gains the entry form, so a contact can be logged from the browser.
  Contacts already in the log can be edited or deleted, and QSL cards marked sent and received. One
  POTA or SOTA activation's ADIF can be downloaded on its own. Awards shows the station's
  confirmation diagnostics, so "why is this one not confirmed yet" can be answered away from the
  radio. Editing and deleting change the permanent record, so they are station-control only.

  **Activations, and spotting yourself.** An activation can be started and ended from the browser,
  stamping the reference on every contact, and another operator's can be hunted; the station's park
  directory is searchable while you log. Nexus can post **your own spot to pota.app and to the DX
  cluster**, so chasers find you without you leaving the radio. Posting to POTA is on and the cluster
  half is off until you turn it on — and **every spot asks first, because it goes out publicly under
  your callsign.** Nothing is ever posted on a timer or in the background.

  **Being told things.** Optional browser alerts for a new need on the station board, for rare DX,
  and for a new POTA activation. Off until you turn them on, and they only notify — **nothing moves
  the radio on its own.**

  **Settings and housekeeping.** A safe subset of operating preferences, Units among them, can be
  changed from the hosted Settings page rather than only read. A received SSTV picture can be saved
  to the browser. Credentials, licence class and audio or CAT device configuration are deliberately
  not there and never will be.

- **A browser approval renews while you use it, up to 90 days from the shack approval.** An approval
  used to run out on a fixed clock whether you were using it or not. It now renews each time the
  browser connects, to a ceiling of 90 days after the approval you gave at the shack, and both the
  browser and the station warn you seven days before it lapses — so you are not locked out from
  somewhere else with no warning. Re-approving is unchanged and is still done at the shack.

- **Nexus Remote says what actually happened, in its own words.** A trial that ends mid-session now
  names itself rather than reading as though the shack had gone off the air; there is a Sign out on
  both the workspace and the station monitor; and when a pairing is gone, the card at both ends says
  why rather than simply failing.

- **Correct one contact at QRZ.** Phone contacts already in your QRZ logbook carry no sideband,
  because Nexus never recorded one until now — and an ordinary push cannot repair them: QRZ sees
  the contact is already there, answers "duplicate", and keeps its copy. Each row in the logbook
  now has a **QRZ✎** button that overwrites QRZ's copy of that one contact.

- **Work a satellite pass from the browser.** Nexus Remote could already show you the passes, the
  schedule, a bird's transmitters and a live track — but every control in the Satellites section
  was greyed out, so you could watch a pass and do nothing about it. From a browser you can now
  arm and stop a track, pick a transponder or hand the dial back, lock back onto the bird, turn
  Doppler on, choose and confirm which VFO carries the uplink, pin the radio, and fetch fresh
  orbital elements — the same controls, doing the same things, as at the shack.
- **Remote Stop also ends a satellite track.** A browser has one Stop, and a track keeps steering
  the dial and the mast on its own; Stop now disarms it, hands the dial back and halts the rotator
  as well as ending any transmission. A track armed from a browser also ends by itself if that
  browser goes away — the session closes, the lease runs out, or you take the station back at the
  shack — so the radio is never left following a bird for nobody.

  It is deliberately narrow. **One contact at a time** — there is no "fix them all", no bulk
  selection, and nothing runs on a timer or in the background. Nexus first reads QRZ's own record
  back, shows you the callsign, the date and exactly which fields will change, and waits for you
  to say yes. What goes back to QRZ is QRZ's own record with only those fields changed, so
  anything QRZ holds that Nexus does not track is returned to it untouched.

  It refuses rather than guess. Nexus will not send a correction that would change the callsign,
  band, mode or time — QRZ finds the record to overwrite by exactly those four details, so
  changing one would add a second contact instead of correcting the first. It also refuses a
  contact with no time of day recorded, one with no station callsign, one dated outside the date
  range of the logbook your API key opens, and any selection that is not exactly one contact.
  Each refusal says which it was.

  If QRZ's matcher disagrees with ours it adds a second copy rather than overwriting, and QRZ's
  answer says so. Nexus reports that as a **failure**, names the record QRZ just added, and offers
  to delete exactly that record and nothing else — it never deletes on its own. QRZ's delete is
  permanent and there is no undo, at QRZ or here, so take a QRZ ADIF export before your first run.
  Your QRZ confirmations are not at risk from the correction itself: QRZ works those out
  continuously from both operators' records and nothing you upload can set or clear one.

- **Export your channel list to CHIRP or a spreadsheet from a browser.** In Nexus Remote, the
  Program section's *Export CHIRP* and *Export CSV* buttons now work: the station builds the file
  with exactly the same writer the desktop uses, and it lands in the downloads folder of whatever
  machine you are sitting at. The rig name-cap picker beside them works too, because it is part of
  the export. One difference from the shack: the file carries no "data courtesy of" line, because
  the saved channel list does not record which directory each machine came from, and naming one
  would be a guess. Exporting still needs station control, and the buttons stay dark against a
  Nexus at the shack that is too old to offer it.

- **Tidy the channel list from a browser too.** Still in Nexus Remote's Program section: rename a
  channel, move it up or down, drop a row, or clear the list. Each one is a single change the
  station applies to its own file, and it names the row by its channel ID rather than by where it
  sat on screen — so a list that changed at the shack while you were looking at it is refused
  rather than edited in the wrong place. A name is sent when you leave the field (or press Enter),
  not per keystroke; Escape puts it back. Adding channels still happens at the shack: the
  RepeaterBook search needs the station's API key, and importing a CHIRP CSV means sending a file
  up, which Nexus Remote has no path for yet.

- **Click a received SSTV picture to look at it, in its own window.** Clicking a gallery
  thumbnail used to do nothing at all — the gallery offered delete, edit-and-resend and Reveal
  in folder, so examining a picture that had just come in meant leaving Nexus for a file
  manager. It now opens big, in a window of its own that you can park on a second monitor and
  leave open while the next picture arrives. The window shows the mode, the time, the frequency
  and the sender's callsign when their software sent one, and offers Save a copy and Reveal in
  folder. Esc closes it; ← and → step through the gallery. Clicking another thumbnail re-points
  the window that is already open rather than opening a second one. Desktop only for now — in a
  browser the gallery still shows thumbnails.

- **SSTV can send your callsign as an FSK ID after each picture — off by default.** Nexus has
  always *read* the callsign burst that trails a received picture and shown it under the
  thumbnail; it could not send one. Settings ▸ Digital ▸ SSTV ▸ "Send my callsign after each
  picture" turns the transmit half on. It is off to begin with and stays off when you update,
  because it adds about a second and a quarter of key-down to every transmission and nobody's
  over should grow because they installed a new version. Your callsign is still drawn into the
  picture either way — this rides alongside that, for the stations whose software shows the
  trailer. The key-down time on the compose bar includes the burst when it is on.

- **The callsign under a received picture now appears for the long modes too.** Nexus read the
  FSK ID after a Scottie 1 or a Martin, but never after PD-240, PD-290, Scottie DX or Pasokon P7:
  it stopped looking before the burst arrived on any mode with a line longer than about three
  quarters of a second. Those four now report the sender's callsign like the rest.

- **Start an SSTV receive by hand, without waiting for a header** (#202). Tune into a picture
  that is already halfway through, or one whose header was lost to a burst of noise, pick the
  mode beside the Arm button and press Start: the decode begins from the next line. The picture
  comes out straight — the line timing is recovered from the sync pulses the same way it always
  was, so joining mid-line is fine. Two things a header would have told Nexus are yours to
  supply: the mode (nothing guesses it, and you will get the mode you name), and the assumption
  that the radio is tuned correctly. Nothing in the audio can start a receive on its own; only
  this button can. Receive only — it never transmits.

- **SSTV receives two more mode families: Wraase SC-2 180 and Pasokon P5 and P7** (#264). Nexus
  recognises their VIS headers and decodes them like any other mode — the picture paints line by
  line and lands in the gallery with its mode name. Wraase SC-2 180 and Pasokon P5 can also be
  sent; Pasokon P7 cannot, because one P7 picture is nearly seven minutes of key-down and Nexus
  will not key the rig that long for one image, so it is not in the transmit picker. A VIS code
  Nexus still does not know — Pasokon P3, Wraase SC-2 120 — is reported as an unrecognised mode
  exactly as before, rather than being decoded as a neighbour and coming out slanted.

- **Listen to the radio from Nexus Remote.** The browser workspace has a Listen button in its
  header, beside the station-control line. Press it and you hear what the receiver hears; press it
  again and it stops. It is muted until you ask for it, it stops on its own when you hide the tab
  or lose station control, and only a browser that holds station control can start it — a browser
  approved for logging alone cannot listen. A station nobody is listening to sends nothing at all.
  The line beside the button tells you what the audio is doing: connecting, listening, a gap while
  the link is losing packets, or stalled. That distinction is the point. A gap is filled with a
  faint synthetic hiss rather than silence, so a dropping link never sounds like a dead band. If
  your browser cannot play the audio (Firefox on Android, and Safari before 26), the page says so
  instead of offering a button that does nothing.

  **1.12.0 said no audio reaches the browser in either direction.** Receive audio now does.
  Transmit audio still does not: you can hear the station, you cannot speak through it.

- **Every panel you can remove now has an × in its own corner.** Panels could always be taken off
  a screen from the ⊞ Panels menu, but there was nothing on the panel itself to say so — if you
  did not already know the menu was there, there was no way to close anything. Now each removable
  panel carries a small × in its own header: on the FT8 screen that is Band Activity, Rx Frequency,
  Tx Messages, the Call Roster, Stations, the callsign card and the waterfall, and the same goes
  for the CW, Phone, RTTY, PSK, SSTV and JS8 screens. The × does exactly what unticking the panel
  in ⊞ Panels does — same setting, remembered the same way — and ⊞ Panels is still where you put a
  panel back, with Undo one click away if you close the wrong one. Panels that are part of
  something else rather than a panel of their own (the TX meters strip, CW's merged Rig controls
  frame, the log strips) have no × and are unchanged. Phone's Voice Keyer × warns, before you
  press it, that closing the keyer stops a voice message that is playing and throws away a
  recording in progress — the same warning its menu entry has always carried.

- **Click-to-work an RTTY spot from a browser.** Nexus Remote could already work a CW, Phone, FT8
  or FT4 spot from the Needed board or the Spots board; an RTTY one was quietly refused. Clicking
  it now retunes the station to the spot's exact frequency and puts it in the RTTY section, the
  same single change the desktop makes. It is a receive change only: it never enables transmit,
  and the station still refuses it while a transmission is armed. A station running an older
  Nexus simply does not offer the control, so nothing is sent to one that could not do it.

- **The browser shows where the rotator is actually pointing.** Nexus Remote could already point
  the antenna by azimuth, point it at a callsign and stop it, but the heading itself read "—".
  The cockpit strip and the Needed board now show the station's own reading, refreshed while a
  heading is on screen and never when it is not. The station's honesty is kept intact: no rotator
  configured, or a rotctld that does not answer, still reads "—" rather than a made-up bearing,
  and a station running an older Nexus keeps the "—" it has always shown.

- **Spot another station from a browser.** Nexus Remote could already spot your own activation;
  spotting someone else was refused. The Spot button in Operate's roster now works from a browser
  with station control, opening the same review popup the desktop shows and asking once more
  before the spot goes out — it posts publicly from your station's cluster login, so it is
  deliberately not something a logging-only browser can do. If no cluster node is connected the
  browser is told so plainly instead of being left to guess.

- **Delete a received SSTV picture from a browser.** Saving one already worked; deleting it was
  desktop-only, so a browser could fill the gallery and never tidy it. The ✕ on a gallery card now
  works from a browser with station control, with the same confirm the desktop asks for — it is
  permanent, and a received picture is the only copy of what somebody sent. The browser names the
  picture by the card it is looking at and the station finds the file itself; no file path is ever
  sent, and a card the station no longer has is refused rather than guessed at.

- **Nexus now knows which parks and summits you have already worked, and when.** A park you have
  not worked is a genuine need on the Needed board and in the Call Roster — a NEW PARK chip
  alongside the award chips, with its own filter chip on the board — so an activator you have
  worked before still stands out when they are somewhere new. It works off the reference your log
  already stores for every hunted contact, POTA and SOTA alike, so there is nothing to set up.

  The board also stops nagging. A park you have already worked in the activation that is running
  now drops to the bottom and keeps only its POTA/SOTA chip: you logged them, you are done with
  them. Tomorrow is a different matter — an activator who goes back to the same park is a fresh
  contact for a hunter, so the park is needed again. The line between "this activation" and "the
  next one" is UTC midnight, the same boundary POTA credits an activation on and the same one the
  activation export already splits your log at, and it counts each activator separately: two
  operators at one park on one day are two visits and two contacts to be had.

  Two things it cannot see, both honest gaps rather than bugs. A hunt made in a mode that never
  passes the reference over the air — CW, usually — leaves nothing in the log to go on, so the
  park keeps reading as needed; the imported Hunted Parks.CSV cannot fill that in because it has
  no dates, though it still drives the hunter panel's NEW PARK badge as before.

- **Stop transmitting when SWR is high** (Settings ▸ Radio ▸ Transmit limits & sharing). Off by
  default. Turn it on and two readings in a row above your threshold — 2.5:1 unless you change it
  — stop the transmission exactly as Stop TX does, and Nexus tells you why. It never starts a
  transmission and never turns TX back on: that is yours to do once the antenna is sorted out. A
  single high reading is ignored, so a tuner stepping or a keyup transient will not cut you off.

  **The switch is greyed out on most radios, and that is on purpose.** Nexus only trusts an SWR
  figure where it knows the radio's own scale — Icom over native CI-V, and FlexRadio. Everywhere
  else the number arrives with no scale Nexus can vouch for and can be far out: some radios show a
  near-perfect match on their own meter while reporting a fault here. A cut-off driven by that
  would take you off the air for nothing, so it is not offered.

- **The band dropdown shows how each band is doing.** In every cockpit (Phone, CW, Operate, RTTY,
  PSK, SSTV, JS8 and Tempo) and the top bar, the band list now shows a coloured dot and a word
  beside each band: Open, Marginal or Closed. It uses the same data as the Band conditions strip
  on the map. If there is no recent data for a band, it shows a grey outline and "No data", never
  green. The dropdown is now a Nexus menu rather than the system list, so it looks the same on
  Windows, macOS and Linux (including the dark theme on Linux, where the system list was drawn
  light). It works from the keyboard and reads out each band and its condition to a screen reader.
  In the Remote browser page the menu works the same way; the dots stay grey there, because the
  browser does not receive propagation data yet.

- **Optional local-time clock beside UTC** (#253). Settings ▸ Appearance ▸ Workspace ▸ "Local time
  beside UTC" adds a second clock in the top bar showing this computer's local time. It is off by
  default and remembered per computer. Logs, spots and FT slots still use UTC.

- **Band Activity can show the newest decodes at the top** (#276). A new "Newest on top" chip in
  the Band Activity pane draws the newest period first and keeps the pane following the top. It is
  off by default, so the pane keeps the WSJT-X order unless you turn it on. If you scroll down to
  read, new decodes arrive above without moving what you are reading; scroll back to the top to
  follow again. It applies when the pane is sorted by time.

- **The CW screen has a His call field** (#286). It sits at the start of the send row and shows
  the station you are working; a decoded call fills it in. Type over it to answer a station the
  decoder missed or misread, then press Enter or a macro key: the `!` in your macros sends the
  call in the box.

- **SSTV pictures paint while they arrive, and the gallery has a Reveal button** (#130). A
  received picture used to stay black until the last line (about two minutes for Scottie 1) and
  then appear all at once. Now each line shows as its audio comes in. If the sending station's
  timing is slightly off, the picture may lean a little while it arrives; it straightens when the
  picture completes, and the saved picture is the corrected one. The Gallery pane also has a Reveal
  button that opens the folder the pictures are saved in (Pictures/Nexus SSTV).

- **You can choose where your log and data are kept** (#289). Settings ▸ Config ▸ "Data & log
  folder" points Nexus at another folder — a NAS or a synced folder, so a second computer in the
  shack can reach the same log. Either adopt a folder that already holds a log, or have Nexus copy
  your logbook, data tables and Winlink mailbox across; a copy is checked file by file against the
  original. **Nothing is ever moved or deleted** — your old folder is left exactly as it was — and
  the new folder is used the next time Nexus starts, never half-way through a session. Nexus
  refuses a folder that would open an empty logbook unless you asked for the copy. One warning
  worth repeating: only one Nexus should use a synced folder at a time, or you get a conflicted
  copy instead of a merged log.

- **You can hide the Logbook globe (D#278).** Settings ▸ Appearance ▸ Workspace has a new Logbook
  globe switch. Turn it off and the Logbook table starts at the top of the screen. It is on by
  default.

- **FT8 and FT4 contacts the sequencer logs now carry the other operator's name.** The log strip
  in Phone, CW and the manual log form already filled Name from the callbook, but a contact the
  FT sequencer logged for you went into the Logbook with no name, even when the callsign card had
  just shown it. Nexus now keeps the name from any QRZ or HamQTH lookup it already did this
  session (the card's own, or the log strip's), QRZ nickname first like the log strip, and puts it
  on the logged contact, its ADIF and its uploads. It never looks a station up just to log it; if
  nothing was looked up, the name stays blank as before. (#293)

- **Filter spots by where they were spotted from.** The Spots panel's filters now include the
  continents and the countries of the stations reporting each spot, so you can keep "spotted
  from Europe" or "spotted from France" only. A spot stays if any station that reported it
  matches, and the chips only offer places present in the current feed. "Heard on my continent"
  is unchanged and still on by default. (#174)

- **Hide whole continents from FT8/FT4 Band Activity.** The Countries picker in Band Activity now
  has a "Hide whole continents" submenu beside its 18 quick picks, so "only Europe and Asia" no
  longer means ticking country after country. It works like the country ticks: stations calling
  you, the one you're working and new entities or band slots still show, Pause keeps your ticks,
  and the hidden-count chip now counts continents (and countries picked by name, which it
  missed before). (#229)

- **More in the Logbook: your own grid and rig on each contact, QSL status in the edit form, and
  a wide table.** Each contact can now carry your own grid square and the rig you used, and both
  go out in ADIF (MY_GRIDSQUARE, MY_RIG) and come back on import. The rig fills itself from the
  active radio when a contact is logged; your grid is yours to enter, for the contacts made away
  from home. Editing a contact now also lets you mark a QSL card sent (bureau, direct or
  electronic) and a card received. A new "More columns" button above the log shows your grid,
  rig, name, QTH, state, power and operator in a wider table that scrolls sideways; it is off
  until you turn it on, so the log looks the same as before. (#239)

- **The callsign card is a panel of its own in Operate.** It has its own entry under ⊞ Panels, so
  you can hide it or keep it, and hiding the Stations list no longer takes the card with it.
  Pressing S&P clears the card, as F4 does. And when the card is about a station that is calling
  someone else, it shows "Calling" and that call: click it to see the called station's card. (#204)

- **Cloudlog and Wavelog can tell Nexus your station locations.** Settings ▸ Logging & Connectors
  ▸ Cloudlog / Wavelog has a **Find my station locations** button beside the station profile id.
  Press it and Nexus asks your own instance which locations it has, lists them by number, name,
  callsign and grid, and fills the number in when you pick one. It asks only when you press it —
  never on its own — and it goes to your instance over https only. An older Cloudlog without that
  endpoint says so and asks you to enter the number by hand. (#226)

- **Satellite contacts no longer carry a satellite name LoTW won't accept.** Nexus worked the
  name out from the bird's catalog name, which produced plenty LoTW has never heard of — GO-32,
  AO-95, IO-26, and cubesats like CUBY-1 whose names only look like an OSCAR number. LoTW refuses
  a contact like that, and it can take the rest of the upload with it. Nexus now stamps a name
  only when it is one LoTW takes, and leaves the field empty otherwise, which you can still fill
  in by hand. Contacts through the ISS now carry ARISS, the name LoTW wants, and the TEVEL-2,
  TAURUS and SONATE birds carry theirs. (#296)

- **Pick your data and log folder instead of typing it.** Settings ▸ Config ▸ Data & log folder
  now has a **Browse…** button that opens your computer's own folder chooser and fills the box in
  for you. Typing or pasting a path still works exactly as before — that is the way to reach a
  network share like `\\nas\ham\nexus`, which a folder chooser will not always show you. Browse
  only fills the box: nothing moves until you press *Use this folder* or *Copy my log and data
  there*, same as always. (#289)


### Changed

- **A release can no longer be published from a commit whose tests never passed.** The release
  pipeline now refuses to publish unless the exact commit being released has a completed,
  successful CI run — not merely one that did not fail. A cancelled or still-running run does not
  count, and neither does a green run on a parent commit. 1.12.0 went out on a cancelled run; that
  cannot happen again. Break-glass releases are still possible and now leave a record of which
  release skipped the check.

- **The callsign card no longer squeezes Band Activity off the screen.** Click a station with a
  long history — eleven previous contacts was the case reported — and the card grew until it took
  half the right-hand rail, leaving Band Activity a couple of rows and shortening Rx Frequency to
  match. The card is now capped at about a third of the rail, and the list of previous contacts
  scrolls inside it instead of pushing its neighbours around; nothing is lost, you just scroll for
  the older contacts. Measured at 1024×768, 1366×768, 1600×900 and 1920×1080 and at 100 %, 110 %
  and 125 % zoom: at 1920×1080 Band Activity goes from 197 px to 258 px and Rx Frequency from
  128 px to 166 px with the card up.

- **Tune is always right next to the band picker** (#287). In Phone, CW, RTTY, PSK, SSTV and JS8,
  the Tune button (and the ATU button, on radios that have one) now sits directly after the band
  dropdown. It used to be at the far right of the header and moved depending on what else each
  mode showed there. Stop TX and the CAT indicator stay where they were. Operate and Tempo are
  deliberately unchanged: those two screens keep Tune where it has always been — in Operate's
  QSO strip and in Tempo's top bar — rather than gaining a second one.
- **Tuning steps land on round numbers** (#273). When the dial is between steps (after clicking a
  spot, typing a frequency or turning the radio's own knob), the first mouse-wheel notch or ◄/►
  click now rounds to the step, the way a radio's VFO does: 18.110.250 at a 1 kHz step goes to
  18.111.000, not 18.111.250. After that each click is a whole step. Hovering a single digit of the
  frequency still moves just that digit.

### Fixed

- **No more command-prompt window flashing on the CW screen.** A brief black command window
  appeared over Nexus when you moved to CW and started decoding — once per run, at whatever
  moment the AI CW decoder first looked at the audio, which made it feel random. It was not Nexus
  itself: the maths library the AI decoder uses asks Windows about the CPU cache by running
  `wmic`, and that tool opens a console window of its own. Nexus now settles that question at
  startup, before there is any window to flash over, and the AI decoder is unaffected. The 1.12.0
  clock-check flash was a different cause with the same symptom and was fixed separately; this is
  the one that was left.

- **Remote's Stop TX still works after your control lease has run out.** If the lease ticked over
  while you were watching the station transmit — a slow link, a tab left in the background — the
  next press of Stop TX was refused and the rig stayed keyed. Stopping is the one thing that must
  never be refused, so a browser that held station control can now always stop the station, lease
  or no lease. An unnecessary unkey is a far smaller problem than a radio you cannot stop. A
  browser that never had station control still gets nothing: no lease, a logging-only lease, a
  revoked grant or a different device are refused exactly as before, and a Stop can still only
  stop — it can never start, arm or re-arm anything.

- **Remote's Stop TX says "Stop sent" until the station confirms the transmitter is free.** The
  station answers a Stop the moment it accepts it, and if it is busy (a decode pass, another
  command) the actual unkey happens a moment later. The browser now shows "Stop sent" until the
  station's own reading shows the transmitter free, and only then "Stopped" — it will not tell you
  the rig is off the air while it is still transmitting.

- **A Windows or Linux build that lost its update signature can no longer be published.** If the
  signing step produced no signature, the release went ahead anyway behind a one-line note in the
  build log — and the result is invisible from the app: the update manifest simply omits that
  platform, so "Check for updates" finds nothing, forever, and reports no error. macOS has
  refused this since it shipped; Windows and Linux now refuse it too.

- **Remote: ending someone's access now stops them commanding the radio straight away.** Access was
  checked when a browser session opened and never again, so a session that was already running kept
  full control of the radio for up to a minute after the trial was ended or switched off — it only
  stopped when the session's own lease ran out. A command that changes the station (a frequency or
  mode change, a setting, a log write) is now checked against live service access before it is passed
  to the shack, and is refused within about two seconds of access ending. Watching is unchanged, and
  **Stop is never refused**: an operator whose access has lapsed must always be able to unkey a
  transmitter, so Stop — and the station read it is built from — still goes through.

- **The callsign card names the state (#237).** A US or Canadian station's card showed the town,

- **Running two Nexus builds on one computer no longer erases settings.** If you run a tester build
  alongside the public release — both use the same settings file — then anything you configured in
  the newer one was silently deleted the next time the older one saved. It kept only the settings it
  recognised and wrote the rest away: no error, no warning, nothing to tell you it had happened. An
  older build now carries settings it doesn't understand through untouched. It still can't show them
  to you (it has no screen for a setting it doesn't have), but it can no longer destroy them, so
  going back and forth between two builds is safe.

- **Two Nexus instances saving settings at the same moment can no longer corrupt the file.** Both
  wrote through the same scratch file, so their writes could land on top of each other and publish a
  half-and-half settings file. Nexus reads that as damaged, sets it aside and starts from defaults —
  which means your callsign, your radios and your licence class all reset. Each instance now uses its
  own scratch file; whichever saves last wins, whole and intact.

- **A logbook rewrite survives a power cut.** Rewriting `log.adi` (after a confirmation merge, a
  mark-all, or a one-time cleanup at startup) wrote the new file and immediately swapped it in
  without waiting for the disk. Lose power in that gap and the computer could come back with the log
  pointing at a file that was never written — every contact gone. The swap now waits for the data to
  be on the disk first. Nothing changes in normal use; this is only about the pull-the-plug case.
  the grid and the country but never the state, so a Hawaii station read "KEKAHA (BL01dx) · United
  States" while the Needed board and Worked All States already knew it was HI. The card now reads
  "KEKAHA, HI (BL01dx)", from the same resolved hint the award maths uses — so the two can never
  disagree — and stations elsewhere are unchanged.

- **Phone contacts now record which sideband you worked them on.** Every phone QSO was logged
  as plain `SSB`, so nothing Nexus wrote down — your own Logbook, the ADIF export, or the
  uploads to QRZ, LoTW, ClubLog, eQSL and the rest — said whether the contact was upper or
  lower. The Logbook's Mode column now reads USB or LSB, the way HRD and the other loggers
  show it, and the entry strip's "Logs to the shared logbook as …" line says which before you
  commit the contact.

  **In the ADIF it goes where the standard puts it**: the mode stays `SSB` and the sideband
  rides as the `SUBMODE`, which is what ADIF has specified since 2013 and what LoTW accepts.
  Writing USB or LSB into the mode field instead is what gets a record thrown out by LoTW, so
  Nexus does not do that — and it no longer does it for *imported* contacts either. A log
  brought in from a program that writes the sideband in the mode field (Log4OM and N1MM both
  do) used to be exported straight back out that way, and every one of those records was
  rejected on upload without ever saying so.

  **The sideband comes from your radio or not at all.** Nexus writes one only when the rig has
  actually told it over CAT which sideband it is on. Without CAT, or with the radio sitting in
  CW or a data mode, the contact logs as plain SSB exactly as before — it will not guess a
  sideband from the band, because a wrong one in a permanent record is worse than none. AM and
  FM are modes in their own right, not sidebands, and are unchanged.

  **This fixes contacts from here on.** Phone QSOs already in your log keep the SSB they were
  written with — nothing goes back and guesses a sideband for a contact that is already made.
  Duplicate checking still treats USB, LSB and SSB as one mode, so nothing you have already
  worked stops counting as worked, and re-importing an old log still adds no second copy.

- **A satellite you just opened no longer claims it has no transmitters.** Open a bird the
  Satellites section had not looked up yet (CO-57, say) and its transponder list read "no
  transmitters listed for this bird" for up to half an hour, because the SatNOGS lookup for it
  waited out the pause after the favourites lookup that had just run. Nexus now looks a newly
  opened bird up straight away, and until the answer arrives it says the transmitters are not
  fetched yet. "No transmitters listed" now only shows for a bird SatNOGS really lists nothing for.
  The Doppler line also told you to pick a transponder "below" when the list is beside it. (#269)

- **On a first launch the Needed window waits for the setup wizard (#240).** The Needed board used to
  open in its own window as soon as Nexus had data, landing on top of the setup wizard. It now opens
  once the wizard is finished or skipped. Later launches are unchanged.
- **Error messages stay on screen long enough to read (D#19).** An error pop-up used to vanish
  after four seconds, or less, often before you looked back from the rig. Errors now stay at least
  twelve seconds, or until you close them. Ordinary notices are unchanged. Because they stay
  longer, notices no longer swallow clicks: click straight through one to the control underneath,
  and close it with its own × (swiping a notice away is gone — the × replaces it).
- **The Rx Frequency pane lets go of −B4 without a restart (#268, #235).** The pane now has its own
  −B4 chip, and turning −B4 on or off in either Band Activity or Rx Frequency changes both panes at
  once. Before, the Rx Frequency pane picked up −B4 when it opened and kept hiding a worked
  station's RR73 until Nexus was restarted.
- **The callsign card updates when the contact is logged (#282).** On the FT8/FT4 screen the card kept
  showing "New DXCC!" (or a new band or mode) for a station you had just worked, until you picked a
  different station. It now re-reads your log the moment the QSO is logged and shows it as worked.
- **Re-docking the waterfall closes its pop-out window (#263).** Clicking re-dock put the waterfall
  back on the FT8/FT4 screen but left the torn-off window open, so you had two. The outside window
  now closes when you re-dock.
- **Journey's Personal bests follows your Units setting (#244).** The longest-distance record was
  always shown in miles. It now reads in kilometres or miles, like every other distance in Nexus.
  Best miles-per-watt keeps its name and unit, since that is the award's own measure.
- **On WSPR the waterfall and the RX marker stay in the WSPR sub-band (#101).** WSPR signals only
  live in the 200 Hz around 1500 Hz, but the waterfall still showed the whole passband and the green
  RX marker could be dragged anywhere in it. While WSPR is selected the waterfall now shows just
  1400–1600 Hz (the zoom picker steps aside, and your zoom comes back when you leave WSPR), and the
  RX marker stays inside that window, as the transmit marker already did.
- **Long logbook comments can be read in full (#162).** The Comment column still shows one line, but
  clicking a comment now opens it to its full length in that row, and clicking again folds it back.
  The columns stay where they are.
- **Waterfall frequency numbers grow with the UI scale (#215).** On Windows the numbers along the
  bottom of the waterfall, and the RX/TX labels, stayed tiny at any UI scale. They now grow with
  Settings ▸ Appearance ▸ Workspace ▸ UI scale, so 110 % or 125 % makes them easier to read.

- **The RTTY, PSK, SSTV and JS8 waterfalls say when the picture is held (#230).** While you
  transmit, Nexus keeps showing the last real picture of the band instead of the muted receiver the
  radio hands back — but on these screens a held picture looked like a waterfall that had died. A
  "TRANSMITTING — display held" badge now sits on the waterfall while you key and clears when you
  stop. Nothing about transmitting changes, and your own signal is still not drawn.

- **A QRZ or eQSL upload that runs out of retries now says so.** After 20 failed tries Nexus used
  to drop the contact from its upload queue without a word, so a QSO that never reached QRZ or
  eQSL only showed up missing months later. The Connections log now names the contact (call, band,
  mode, date and time) and says how to send it again: the QRZ button on its Logbook row, or Push to
  eQSL under Awards ▸ Confirmations. (#290)

- **Log4OM set to read WSJT-X's ADIF message now gets Nexus contacts.** Nexus sent the "QSO
  logged" message over WSJT-X UDP but never the separate ADIF message WSJT-X sends right after
  it, so a logger listening only for the ADIF one showed the callsign during the QSO and never
  received the contact. Nexus now sends both, in WSJT-X's order; the "QSO logged" message itself
  is unchanged, so JTAlert, GridTracker and loggers already working carry on as before. Field Day
  contacts still send only the "QSO logged" message. (#267)

- **The Logbook's push-to-QRZ button says QRZ.** Each row's QRZ upload button was a bare ↥ arrow,
  easy to take for something else, so it looked as if uploading a single contact to QRZ had gone.
  It now reads QRZ, like the CL, HL and WRL buttons beside it; hovering still says what it does,
  and screen readers still hear "Push <call> to QRZ". (#270)

- **The portable-callsign warning now appears when you upload, and eQSL has one too.** Nexus
  warned that a QRZ logbook belongs to one exact callsign only when you pressed Test Connection,
  so an operator set up at home who went portable months later was never told why his /P uploads
  failed. Now the first QRZ upload under a callsign that doesn't match the logbook writes the
  warning to the Connections log. For a portable call Nexus asks QRZ once per session which
  callsign the logbook belongs to; an ordinary call costs no extra request. eQSL uploads check the
  callsign against your eQSL username the same way. Each warning appears once per session, not
  once per contact. (#291)

- **Wavelog and Cloudlog: the station profile id is explained, and a callsign there is caught.**
  Wavelog wants the station location's number, the one at the end of that location's Edit link
  under Station Locations; one operator typed his callsign and Wavelog refused every contact
  with "station id does not belong to the API key owner". The hint now says what the number is
  and where to find it, a value that isn't a number gets a note right under the field, and Nexus
  no longer sends an upload with one. (#226)

- **A Wavelog or Cloudlog upload that is refused is no longer retried over and over.** Every
  Cloudlog/Wavelog error used to count as temporary, so a contact the server refused (a wrong
  key, a callsign in the station profile id, a URL that isn't the API) was sent again and again
  and failed the same way each time. Now only "couldn't reach it" and server errors are retried.
  A refusal is logged once in the Connections log, naming the contact, what to fix in Settings,
  and how to add that contact afterwards with Logbook ▸ Export ADIF. (#226)

- **Units is on the Station tab, where you'd look for it.** The km/miles and °C/°F choice applies
  to the whole app but was filed under Settings ▸ Digital ▸ Station Housekeeping, beside FT8
  options. It now sits under Settings ▸ Station ▸ Operator & Radio, and typing units, miles, km,
  imperial, metric or temperature into the settings search goes straight to it. Your existing
  choice carries over. (#248)

- **Nexus Remote: the rest of the controls stop greying out on a slow link.** 1.12.0 steadied the
  band control and its neighbours when the link to the shack hiccups. The amplifier's Operate and
  band buttons, the decode-depth chips, the RX offset box and the receive-gain slider were still
  reading a different clock — the age of the station's own rig readings — and on a real internet
  link that ticks past its limit most seconds, so those controls flickered between live and greyed
  out under your hand. They now stay live for as long as this browser holds station control, the
  same rule everything else in the workspace follows.

  Nothing was loosened about when the station will actually act. A control is still dead when you
  do not hold station control, when the station does not offer it, when the rig is keyed, and when
  the shack has genuinely gone quiet — after five seconds without a reading the workspace still
  dims and everything stops, exactly as before. A command made during a hiccup still waits up to a
  second and a half for the link and then reports "Not sent" rather than arriving late, and Stop TX
  is still always live.

- **XIT is counted when Nexus checks your licence privileges.** The key-time check judged the dial
  (and a confirmed split) and stopped there, so an XIT offset big enough to carry the transmitter
  out of your segment still keyed. It is now judged at the frequency XIT actually transmits on, in
  both directions: an offset that moves you out of privileges locks transmit, and one that moves
  you *into* them no longer refuses a legal over. RIT is untouched — it moves the receiver. If both
  split and XIT are on, Nexus requires both candidate transmit frequencies to be legal, because
  whether a given radio adds XIT on top of its split VFO differs by radio. Nexus can only see an
  offset **it** set: one dialled on the radio's own clarifier knob is invisible to the check.

- **Tuning an FM repeater checks your privileges at the machine's input.** A repeater is worked on
  its input, and nothing was asking whether you may key there — the licence check that follows
  looks at the dial, which is the machine's *output*. A machine whose input falls outside your
  privileges is now refused, and the message names the input frequency. The check is on the input
  carrier: a machine whose input sits within a few kHz of a segment edge is still your call.

- **XIT and VFO changes are held back while the radio is transmitting.** Both move the transmit
  frequency — a VFO change hands the transmitter to a different VFO mid-over — and they were only
  being held back on the radio's own PTT read-back, which is a poll and can be up to a second
  behind the key. They now also stand down when Nexus can tell from the frequency that the rig is
  keying (a satellite pass reporting its uplink). Nothing is lost: the change you asked for is
  held and goes out on the first tick after unkey. RIT is unaffected — it moves the receiver.

- **After Tune, Nexus gives back the power the radio is actually on (#234).** It used to restore
  the last level *it* had sent, so a level you set on the radio's own control was quietly replaced
  every time you tuned. Nexus now asks the radio what it is running at just before the tune keys,
  and hands that level back afterwards — which also means the tune level is the lower of your tune
  setting and the power really in use, rather than a stale one. A radio that will not report its
  power is left alone entirely: with nothing to put back, nothing is taken away.

- **A scope span the radio refuses now says so (#275).** On an IC-7300 with its scope in Fixed
  mode a span set is rejected, and the rejection was being dropped in three places — so the span
  buttons did nothing at all, with no explanation anywhere. The refusal is now shown in the status
  lane, naming the span and what to do about it, and it clears itself the moment a span is
  accepted. **Nexus does not change the radio's scope mode for you:** Fixed is a deliberate choice,
  and flipping it to make a button work would be a bigger surprise than the button not working.

- **OmniRig: the dial is read from VFO A/B when the rig file has no plain frequency (#144, #161).**
  OmniRig only fills its `Freq` property from a rig file that defines one; plenty of rig files
  report the dial only through `FreqA`/`FreqB`, and for those Nexus read zero — no dial at all —
  while other programs on the same OmniRig slot were fine. Nexus now falls back to VFO A, then VFO
  B. If none of the three carries a frequency, that is a CAT error — never a radio shown sitting
  at 0.000 MHz. On a radio parked on VFO B whose rig file reports both, VFO A is still what Nexus
  reads.

- **A mode change that ends up on the radio's own wide filter now says so (#82).** When a radio
  keeps refusing a mode with an explicit filter width, Nexus falls back to setting the mode at the
  radio's *default* width to get the mode accepted at all — which on a Flex is 6 kHz, far wider
  than FT8 wants — and then puts the width it asked for back. It was taking the radio's "OK" for
  that second step at face value, and a Flex answers OK while keeping its own filter. Nexus now
  reads the width back and, if the radio is still on the wide one, says which filter it is on and
  which one to set by hand. A radio that will not report its width is left alone rather than
  warned about.

- **"Hold the data mode while SSTV is receiving" now covers HF, not only FM (#191).** The
  per-radio switch added in 1.11.1 held FM-D on an FM channel and did nothing at all on HF, where
  most SSTV is worked — so a station on 14.230 kept dropping out of USB-D between pictures, which
  is exactly what the switch exists to stop. One switch, both classes: FM-D on an FM channel,
  USB-D/LSB-D on HF. It is still off by default and still per radio, and the mic-jack opt-out
  still applies. The switch is renamed accordingly. **Stop the receiver before you go back to
  voice** — while it runs, transmit audio comes from the data port on HF now too.

- **Tune no longer keeps a finished FT contact alive.** If you called a station, it stopped coming
  back, and Nexus gave up on it, running the ATU used to restart the clock that was the only thing
  ending that contact — and the station could "answer" minutes later out of a period the tune had
  cut short, putting you back on the air with nothing clicked. Coming off Tune now switches TX off
  in the digital section, which is what WSJT-X has always done, and drops the decoder's memory of
  who you were working. Your microphone is untouched: Phone, CW, RTTY, PSK and SSTV keep the TX
  switch they had, and the radio's own ATU button is unaffected.

- **A directed CQ typed the wrong way round is refused, not quietly turned into a plain CQ.**
  Typing something like `CQ KD9WES POTA` into the Tx6 box used to put a bare `CQ KD9WES EN61` on
  the air while the box went on showing what you typed. Nexus now transmits nothing and tells you
  why, and where it can work out what you meant from your own words it names the form that would
  work — `CQ POTA KD9WES`. It does not rewrite the box for you and it does not change what a CQ
  may look like: the message that goes on the air is always the one on the screen.

- **"Hide worked" stops hiding park and summit activators.** If you had ever worked a callsign —
  once, years ago, on any band — the Call Roster's Hide worked dropped that station even while it
  was on the air right then from a park or a summit. The Needed board listed the activation and
  the roster threw it away, so the two panes disagreed about the same station at the same moment.
  Hide worked now keeps a station that is at a park or summit you have not worked in the
  activation running right now, whatever the log says about the call — and it is the same fact
  the Needed board is reading, so the two panes cannot drift apart again. It still hides a worked
  station with nothing to offer, and a DXpedition you have already worked on the band still does
  not count as a reason to keep the row — that is a label, not something you can need.

### Changed (60 m)

- **The 60 m FT8 band button follows your country** (#175). If your callsign is a US one, 60 m
  still goes to 5.3715 MHz, the US channel. Any other callsign now goes to 5.357 MHz, in the
  worldwide 60 m segment most countries share. A portable call counts where you are operating
  (DL/W1AW is Germany, W1AW/VE3 is Canada). Your own Settings ▸ Frequencies override still wins,
  and the standard table there now shows both dials. 60 m rules differ country to country, so
  check your own band plan and power limit.

### Security

- **Encrypted connections refuse a malformed setup from the server.** Nexus's secure-connection
  library (rustls) is updated to 0.23.45 for RUSTSEC-2026-0285: during setup, a server could send
  messages in the clear that should have been encrypted, and Nexus would not hang up. The
  connection was still verified end to end, so nothing could be read, changed or faked, but it now
  refuses that setup, as the standard requires.

### Fixed

- **Brief command-prompt windows no longer flash up on Windows.** A few seconds after Nexus
  started, and again every ten minutes, three or four black command-prompt boxes could pop up and
  vanish. That was Nexus checking your PC's clock settings (the Windows Time service, its
  registry values, and whether another time program is running). It still checks them, just
  without a window. Every other helper program Nexus starts, such as Hamlib's rigctld and TQSL,
  now opens without a window by the same route, so a new one cannot bring the flashes back.

