# Nexus 1.21.0

POTA activators get a full kit. The activation strip now counts what POTA counts: your contacts from the park since
0000Z, each call, band and mode once, as "US-1234 7 of 10 · 3 to go", with two-fers, a dupe warning for today, the
park's grid on every contact and park-to-park counts. An Activation pane, or a window of its own, shows the
activation running now, and Stop opens an "Activation over?" checklist that exports the ADIF files POTA wants, one
per park per UTC day. Spot me takes a comment, a notice offers a re-spot when you move, and QRT can be posted at the
end. Contacts already in the log can be re-tagged with your park, Awards counts your activator achievements, the map
shows your park, and a contest contact made during an activation counts for it.

Contesting starts with a setup that walks you through it: the contest, what it sends, your entry, your radio and
mode, and Start, which checks the station really entered the contest. Settings ▸ Contesting has Start contest… and
Stop contest… in place of the Contest mode switch. A new Stats tab shows rates, hours, bands, countries and
multipliers live from your contest log. In a contest, FT8 and FT4 Search & Pounce now only listens and you start each
contact with a double-click, and Running stops after each contact once your last over has gone out, as WSJT-X does
with Auto Seq on. The entry shows the country of the call, the last ten contacts with Edit on each, and CQ WW counts
the WAE areas such as Sicily as countries of their own, as its rules do, while the QSO parties count Sicily as Italy.

The Logbook can now be cleaned up and trusted. Import shows what a file holds before anything is written, and
reports every field afterwards. Clean up finds and merges duplicates, bulk edits your own fields, checks the log for
bad grids, bands and references, and fills empty fields from your callbook. Filters and saved filters search the
whole log. Every contact keeps its history, every change can be undone, and Recently deleted brings a deleted
contact back.

Holders of the grandfathered US Novice and Advanced licences can pick their class, and the transmit lockout holds
them to their own segments. On 10 m, a Technician or Novice can send only CW and single-sideband voice from 28.3 to
28.5 MHz. A program keying the radio through Nexus is now refused anywhere its audio could reach past your privileges,
and a split set at the radio holds for a long over.

Also new: WPX, WAE, WWFF, Worked All Provinces, the Fred Fish Memorial Award and custom awards, a station card for any
call, callbook answers kept on this computer, Logbook columns you choose, an SDRplay receiver as the scope's source
through SDRconnect, and JTTY, WSJT-X's new keyboard mode. The SDRconnect scope and JTTY have not yet been tried on
real hardware and on the air.

## The short version

- **POTA activations count as POTA counts:** today's contacts from the park, each call, band and mode once, two-fers,
  "Dupe today" warnings, the park's grid on each contact, park-to-park counts, and an activation that survives a
  restart.
- **An Activation pane and window, and "Activation over?"** at Stop: each park's count, the ADIF files for POTA, Post
  QRT, and reminders.
- **POTA spotting:** a comment on Spot me, a re-spot prompt when you move, and QRT at Stop, on every park of a two-fer.
- **Re-tag contacts with your park** in Logbook ▸ Clean up ▸ Bulk edit, with a warning for contacts already uploaded or
  exported.
- **POTA activator awards:** valid activations, parks, Repeat Offender, Kilo and Rover, from your log.
- **Contest contacts made during an activation carry the park** and count toward it.
- **`{MYPARK}` and `{HISPARK}` CW macros**, and a POTA set of CW keys while you activate.
- **A contest setup in five steps**, from the Contest screen or Start contest… in Settings ▸ Contesting.
- **Start contest… and Stop contest… replace the Contest mode switch** in Settings ▸ Contesting.
- **A Stats tab on the Contest screen:** rates, hours, distance, bands, modes, countries and multipliers, live.
- **Contest FT8 and FT4:** S&P works the station you double-click and never calls by itself, and Running stops after
  each contact until you call CQ again.
- **Contest entry:** the country of the call above the Call box, the last ten contacts, and Edit on each.
- **WAE areas count as countries only in CQ WW**, as its rules say. The QSO parties count Sicily as Italy.
- **Contest over?** exports the Summary, lists the Cabrillo lines the sponsor needs, and shows where the log goes.
- **Import shows a preview** before anything is written, reads ADX and Cabrillo, and reports every field.
- **Logbook ▸ Clean up:** find and merge duplicates, bulk edit, check the log, and update from the callbook.
- **Logbook filters and saved filters**, sorting on up to three columns, and columns you choose.
- **Every contact keeps its history**, every change can be undone, and Recently deleted restores what you delete.
- **Novice and Advanced licence classes**, held to their own segments.
- **Technician and Novice on 28.3–28.5 MHz: CW and single-sideband voice only**, as the FCC rules say.
- **Other programs keying through Nexus are checked across their whole passband**, not only at the dial.
- **A split set at the radio holds through a long over**, for up to 2 minutes.
- **More awards:** WPX, WAE, WWFF, Worked All Provinces, the FFMA, custom awards, a whole-log DXCC chart and What to
  QSL.
- **A station card** for any call, from the Logbook, Spots, the map and the decode lists.
- **Callbook answers are kept on this computer** for 30 days, and callook.info answers US calls.
- **New contacts carry more:** your grid, state, operator and power, start and end times, and what the callbook said.
- **An SDRplay receiver as the scope's source through SDRconnect** (needs a bench check).
- **JTTY, WSJT-X's new keyboard mode**, to receive and send (needs a check on the air).
- **FT8's full decode starts from the audio the early decode has cleared**, as WSJT-X does, and finds every station
  it found before.
- **FT8 and FT4: a call made during the other station's period goes out once**, not twice.
- **Remote:** one Devices list at the shack, one Connect to my station entry, Allow to transmit, and your own
  signed-in devices added without a trip to the shack.
- **Linux:** the PC .deb and AppImage run on Ubuntu 22.04, Mint 21 and Debian 12.

## Upgrading from 1.20

- **Settings ▸ Contesting has Start contest… and Stop contest… instead of the Contest mode switch.** Start opens the
  contest setup and Stop opens the end-of-contest checklist, at once and without Save. Settings no longer saves
  Contest mode on with an exchange the station cannot start with.
- **In a contest, FT8 and FT4 S&P no longer calls stations by itself.** You start each contact with a double-click on
  a CQ, or a reply from a logging program. The contact still runs itself to RR73 and the log. Leaving TX on never
  calls anyone. With **Disable TX after sending 73** on, TX goes off after every S&P contact.
- **In a contest, FT8 and FT4 Running stops after each contact.** Once the contact is logged and your last over has
  gone out, TX goes off and Nexus stops calling CQ. Press **Call CQ**, **Running** or **TX On** to call again. Outside
  a contest, Call CQ still goes back to CQ after each contact.
- **The QSO parties count a WAE area as its DXCC country.** Working IT9 and I in the Tennessee, Texas or Illinois QSO
  Party is one multiplier, not two. CQ WW still counts them as two.
- **Import ADIF opens a preview first.** Choose Import, Import new only or Cancel. A file over 200 MB is refused;
  split it and import the parts.
- **New contacts carry your grid, state, operator and station power.** Your grid and state come from Settings ▸
  Station, the operator is your station call when Operator at the key is blank, and the power is Station power in
  Settings ▸ Digital ▸ Station Housekeeping, when you set one. Your LoTW uploads leave out a grid or state Nexus filled
  in itself, so TQSL signs from your Station Location as before.
- **A contact typed in for an earlier time gets no station details from your current settings.** One that starts
  more than 3 hours before you log it gets no grid, state, operator, rig or power unless you enter them.
- **Phone, CW, RTTY, PSK and Satellites contacts log a start and an end time.** The start is when the call was
  entered; the end is when you press Log.
- **A deleted or purged contact is kept for 90 days** so Recently deleted can restore it. Settings ▸ Config ▸ Data &
  log folder ▸ **Keep deleted contacts for (days)** changes that. Edits are kept for good.
- **Contest contacts merged before this version get the grid, name and QTH their exchange carried**, once, on the
  first start, where their own Grid, Name or QTH is empty. One Undo in Recently deleted takes the whole copy back.
- **The settings file stays readable by older versions.** They see a Novice as a Technician and an Advanced as a
  General.
- **Other programs keying through Nexus's CAT broker or TCI** are allowed only when everything from the dial to 3 kHz
  above it (below on LSB) is inside your privileges. A key already held drops if you retune that close to an edge.
  On 28.3–28.5 MHz a Technician or Novice cannot let other programs key at all, since Nexus cannot hear what they send.
- **The Tempo 23 cm channel moved from 1296.200 to 1296.185 MHz.** Older builds still call on 1296.200; tune there by
  hand to reach them.
- **The phone scope starts on the Fast window** (85 ms) and its trace settles faster. The scope's window button
  still sets Balanced or Sharp, and your choice is remembered.
- **A FlexRadio streams its panadapter at 30 frames a second** instead of 15, roughly doubling the data it sends.
- **FT8's decoder is busy about twice as long after 14.4 s** on a busy band, for its second search of the original
  audio. The few stations only that search finds show about a second later.
- **Empty boxes no longer show made-up examples** such as W1AW, 1D, WI or 192.168.1.50. A box's real default, such as
  the rigctld port, stays.
- **Remote has its own Settings tab**, next to Station. Its card is unchanged and every switch keeps its setting.
- **Settings ▸ Remote access has one Devices list** in place of the separate lists of browsers, computers, paired
  computers and people.
- **On the computer you operate from, Connect to my station… replaces Remote stations… and Stations on this
  network…** The station list, a typed address and the code are under Advanced.
- **Your own signed-in devices are added to your station without approving them at the shack.** A device added this
  way cannot transmit for its first 15 minutes; one on your own network is never held, and Approve or **Allow to
  transmit** at the shack lifts the hold at once. Stop is never held.
- **Nexus Remote is free during the beta**, with no 14-day limit, where the service runs the beta.
- **The old Remote page's FT8/FT4 transmit permission and its remote logging permission are gone.** A browser
  streaming the station operates Nexus as at the shack. A browser that had only remote logging left has nothing:
  approve it again or allow its station controls.
- **Linux PC packages need glibc 2.35**, and now run on Ubuntu 22.04, Mint 21 and Debian 12 (`ldd --version` shows
  yours). The Raspberry Pi packages are unchanged.

## POTA activating

### The activation strip

- **Today's count, as POTA counts it.** The strip on the POTA/SOTA view counts your contacts from the park since
  0000Z, each call, band and mode once: "US-1234 7 of 10 · 3 to go", marked valid at ten. At 0000Z the count starts
  again, the activation keeps running and a notice says so. The Logbook's activation export list counts the same way.
- **Two-fers.** Type two or more parks ("US-1234, US-5678"): each contact is logged once carrying both, each park gets
  its own count, and the export writes one file per park.
- **Dupe today.** While you activate, the log form shows "Dupe today @ US-1234" with the earlier contact's band, mode
  and time when the call was already worked today on that band and mode from the same park of theirs. It only warns;
  the contact still logs.
- **The park's grid.** Your contacts carry the park's grid from the park list as MY_GRIDSQUARE, and you can type a
  more precise one in the strip. Your home grid comes back at Stop. A park with no grid in the list says "No park
  grid: type one".
- **Park to park.** Every POTA activator on the board shows a P2P badge, and the strip counts today's park-to-park
  contacts, park by park on a two-fer. A hunter in two parks counts once toward your ten, and the export writes that
  contact once per park of theirs so each park gets its P2P credit.
- **An activation survives a restart or a crash.** It is taken up at launch paused, with a "Still activating
  US-1234?" notice. Nothing is tagged with the park until you press Still activating, so contacts made at home after
  a day that ended without Stop stay home contacts.
- **A contact waiting in the confirm-before-log pop-up** carries the park, grid and state of the activation it was
  made in, whatever runs when you confirm it.
- **A park typed with the old K- prefix** is accepted, with a reminder to check it before upload.

### The Activation pane and "Activation over?"

- **An Activation box** shows each park's count today, park-to-park counts, how long the activation has run, how long
  until 0000Z, the rate, and this activation's contacts, newest first. Put it in any cockpit, the dashboard rail or
  Conditions, or open it in its own window with **Activation window** on the strip. It sends nothing.
- **Stop opens a checklist before the activation ends:** each park's count, Export the ADIF files for POTA (one per
  park per UTC day, to Downloads), Post QRT once you have spotted, and reminders for a K- park, contacts with no grid,
  a state line with no state, and that you upload the files at pota.app yourself. Nothing is ticked until you tick it,
  and Keep activating changes nothing.
- **More than one call at a park** (KD9TAW and KD9TAW/M, say) is shown with each call's count, because POTA counts each
  call separately, and the checklist lists each call's file.
- **The Logbook's This activation button** shows only this activation's contacts, a two-fer and a run past 0000Z
  included. Filters has a new My park / summit field for your own reference, matched whole.

### Spotting

- **Spot me takes a comment** (QRV when blank, up to 30 characters), sent to both pota.app and the DX cluster. On a
  two-fer one confirm names every park, and each park is spotted on pota.app on its own.
- **A re-spot prompt when you move.** When the dial has sat at least 1 kHz (CW and digital) or 3 kHz (phone) from your
  spot for 20 seconds, a notice says "Moved to 14.062. Re-spot?". Nothing is posted unless you press it and confirm.
- **QRT at Stop.** Once you have spotted, the checklist offers Post QRT, unticked every time, and asks before it posts.
- **The cluster spot fits a node's 30-character comment** by naming later parks as a count ("POTA US-1234 US-5678 +1
  QRT"), never by cutting your comment.

### Your log, the map, awards and macros

- **Re-tag contacts with your park after the fact.** Logbook ▸ Clean up ▸ Bulk edit sets or clears MY_POTA_REF and
  stores a two-fer as an activation does. The preview says which contacts were already sent to LoTW, eQSL, QRZ or Club
  Log (they keep the park they were sent with; nothing is sent again) and which are in an activation file you already
  exported, naming the files to export again. One Undo takes it back.
- **Contest contacts made during an activation count for it.** They carry the park, program, state and the park's
  grid, keep them through the merge into the Logbook, and count toward the ten, the export and the awards. Your
  Cabrillo and the contest ADIF export are unchanged.
- **POTA activator achievements in Awards:** valid activations, days that fell short, parks activated, the next
  achievement, Repeat Offender, Kilo and Rover, each as POTA defines it, with a table of your parks. Each call is
  counted on its own, as POTA counts each account; a portable /P counts as the plain call. pota.app still awards from
  the logs you upload there.
- **Your park on the map.** With the Parks on the air layer on, the park you are activating is a filled, named diamond
  and parks you have activated are outlined. Parks are drawn at their own pin, not their grid square.
- **`{MYPARK}` and `{HISPARK}`** in CW macros, and while you activate and no contest runs, the CW keys you have not
  made your own are a POTA set: CQ POTA, your exchange with state and park, TU 73, and your parks. `{MYSTATE}` sends
  the state your contacts carry.

## Contesting

### Starting and stopping

- **A contest setup that walks you through it.** **Set up a contest…** on the Contest screen, or **Start contest…** in
  Settings ▸ Contesting, opens five steps: the contest (those running or starting this week first, every window in
  UTC); what it sends, showing only the boxes that contest uses and why it cannot start yet; your entry, asking only
  the Cabrillo lines that contest's log writes; your radio and mode, where a mode the contest does not allow is a
  warning with a way to CW or Phone, never a block; and Start.
- **Start checks the station really entered the contest** and says "Illinois QSO Party is on · S&P". If it did not, it
  says why and puts back everything it saved. Start never calls CQ, and Cancel saves nothing.
- **Starting a different contest while one runs** asks you to confirm that it ends the running one; its contacts stay
  apart.
- **Field Day's power is one question**, QRP, LOW or HIGH with ×5, ×2 or ×1 beside it, plus battery or natural power
  for ×5.
- **Change setup… while a contest runs.** A new county, state, class or section goes out through I moved, where the
  contest allows a station to move.
- **A quiet line with Set up…** shows on the Contest screen and the Contests pane when a contest starts within a week.
  There is no pop-up.
- **Contest over? offers more:** export the Summary, a box for each Cabrillo line the sponsor needs that your log
  leaves out, and where the sponsor takes the log, with Copy. Nexus never opens that address or sends the log.

### The Stats tab

- **Stats beside the scoreboard**, live from your contest log: contacts, on time and breaks, the overall rate, your
  best 60 minutes, the current rate, time since your last contact, contacts per UTC hour, your longest and average
  distance, bands, modes, countries, grids, your contest's multiplier board, and the score.
- **Breaks follow the contest's own minimum** (60 minutes in CQ WW and CQ WPX, 30 in Sweepstakes, 30 stated as a
  default elsewhere), and the scoreboard's On and Off time follow the same rule.
- **Share stats** saves a picture to Downloads; nothing is posted. The tab changes no contact and transmits nothing.

### FT8 and FT4 in a contest

- **S&P only listens, as in WSJT-X.** You start each contact with a double-click on a CQ, or a reply from a logging
  program, and S&P then waits for your next pick. In ARRL Field Day and Winter Field Day on FT8, FT4, FT2, FT1 and DX1,
  the double-click answers with your class and section and logs it in the contest.
- **Running stops after each contact**, once the contact is logged and your 73 or RR73 has gone out, as WSJT-X does
  with Auto Seq on. While TX is off the run takes no caller. The AUTO-CQ sign goes out while the run is stopped.
- **The Contest screen says what Running does:** on an FT mode it turns TX on and calls CQ.

### The entry

- **The country of the call** shows above the Call box as the contest counts it, with the usual CQ zone where the
  exchange is a CQ zone ("Sicily · CQ 15" for IT9 in CQ WW). Nothing is filled in for you.
- **The last ten contacts**, instead of five, each with **Edit** to correct it in place, with the dupe marks and the
  score following. An edit is refused at a club and through Remote, and a contact changed in another window closes
  your edit instead of being overwritten.
- **Typing in the middle of a call no longer jumps the cursor to the end**, in the contest entry and every box that
  capitalises as you type.
- **Empty boxes are blank** instead of showing a sample such as W1AW or 1D.

### Scoring and the merge into the Logbook

- **WAE areas count as countries only in CQ WW.** The Tennessee, Texas and Illinois QSO Parties counted Sicily and the
  other WAE areas as a DX multiplier apart from their DXCC country. They now count DXCC countries only.
- **Merging keeps the grid the other station sent**, in the ARRL VHF contests, and a name and QTH sent in an exchange,
  in the Logbook's own columns.
- **WAZ counts the zone a CQ WW contact sent**, and WAS, the statistics and the Logbook show the state a contact's
  exchange carried.
- **A contest's contacts keep their operator, station call, radio, power and grid** in the Logbook, with an end time.
- **The Needed board shows a "show all spots" step** during a contest, one click to the full Spots view.

## The Logbook

### Import

- **A preview before anything is written:** how many records and their dates, how many are new and how many are
  already in your log, the problems found with examples, and the fields the file carries. Then Import, Import new
  only, or Cancel. Undo or Recently deleted takes an import back in one step.
- **ADX and Cabrillo logs** can be imported. A Cabrillo log is read with its contest's exchange template, each column
  going to its ADIF field. A line whose other station cannot be told is listed and not imported.
- **A no-loss report after every import** accounts for every field of the file, with a reason for anything not stored.
  Save it as a text file.

### Clean up

- **Find and merge duplicates:** the same call, band and mode within 5 minutes, or within an hour for possible ones.
  Merging keeps one copy, gives it everything the others held, every confirmation included, and shows a field the
  copies disagree on for you to choose. Nothing is sent to any service, and Undo takes the whole merge back.
- **Bulk edit** sets or clears your grid, station call, operator, rig, power, propagation mode, satellite or POTA/SOTA
  reference on the contacts you tick. It never changes the worked call. Changing the station call clears those
  contacts' upload marks and their LoTW, eQSL and QRZ confirmations, and the confirm says how many before you go on.
- **Check the log** lists invalid grids, a DXCC entity the call's prefix does not match, a band its frequency is not in,
  times that cannot be right, and POTA, SOTA or IOTA references in the wrong format, each with a proposed fix. Only the
  fixes you tick are made.
- **Update from callbook** fills the empty fields of the contacts you pick from QRZ, then HamQTH, then callook.info for
  a US call. Where the station is (grid, QTH, state, zones and so on) is filled only on contacts from the last 12
  months. A field a contact holds is never changed, and nothing is uploaded. It looks one call up every 2 seconds, up to
  250 a day, runs in the background, and Undo takes it back. It follows QRZ's published XML rules, and an account
  without the QRZ XML subscription uses HamQTH and callook.info.

### Finding and seeing your contacts

- **Filters** beside the search box: call or pattern, band, mode, dates, DXCC, continent, zones, grid, state, county,
  IOTA, park or summit, contest, words, QSL state per service and award need. Save a filter under a name.
- **Sort on up to three columns** with Shift-click, and grid, state, IOTA, county and contest sort too.
- **Columns** replaces More columns: show, hide and resize each one. New columns show grid, IOTA, county, zones,
  continent, distance, QSL dates, each service's upload state and the contest exchange.
- **A station card.** Click a call in the Logbook, Spots, the decode lists, the map or the log strip's new Card button:
  distance and bearing, your contacts as a band × mode grid, what it would be new for, its latest spot, and Lookup for
  the callbook. Opening it asks nothing over the internet, and it never covers the TX strip or Stop TX.

### History and undo

- **Every contact keeps its history:** each change, when, what it changed and who made it, with **Undo** on each.
- **Recently deleted** lists deleted contacts with **Restore**, and recent imports and purges with Undo, each asking
  first. A whole import or purge undoes in one step and never touches a contact logged after it.
- **An undo never sends anything** to QRZ, Club Log, eQSL or LoTW. An undo that changes a callsign marks the contact to
  be sent again, as correcting a callsign does.
- **A large undo waits while you transmit**, because it holds up the radio while it is made.

### What each contact records

- **Your grid, state, operator and power** on every new contact, and **start and end times** on Phone, CW, RTTY, PSK
  and Satellites contacts.
- **What the callbook said** when you looked the station up: county, IOTA, zones, position, e-mail, web, QSL manager
  and their LoTW, eQSL and card habits. Nothing you typed is replaced.
- **Callbook answers are kept on this computer** for 30 days (adjustable), and offline at any age. No password or
  sign-in key is kept.
- **LoTW confirmations fill** the station's grid, county, IOTA, ITU zone and prefix when the contact had none.
- **The continent and distance** of each contact, and, with a new switch, **space weather** (K index, A index, SFI).
- **A Comment box in the Log this QSO? pop-up.**
- **A TCI radio's contacts name the radio**, such as SunSDR2DX, instead of "TCI".

## Licence classes and transmit checks

- **Novice and Advanced licence classes**, in the setup wizard and Settings ▸ Station. A Novice is held to CW on 80,
  40 and 15 m, 28.0–28.5 MHz on 10 m (data below 28.3, single-sideband voice above), 222–225 MHz and 1270–1295 MHz; an
  Advanced to a General's privileges plus the Advanced phone segments on 80, 40, 20 and 15 m. Power limits are still
  yours to keep: the lockout checks frequency and mode, not power.
- **Technician and Novice: only CW and single-sideband voice on 28.3–28.5 MHz.** AM, FM and SSTV pictures were let
  through there, which 47 CFR 97.307(f)(10) does not allow. They are refused now, and the refusal says why.
- **Outside keys are checked across their whole passband.** A key from another program through the CAT broker or TCI
  is allowed only when everything from the dial to 3 kHz above it (below on LSB) is inside your privileges for the
  mode, so a data program at 1500 Hz can no longer send past a band or segment edge from a dial just inside it. 60 m
  channels are judged at the channel, and in a CW-only segment other programs key only while the radio is in CW.
- **A split set at the radio holds through a long over.** With Follow the radio's split on, the last reading now holds
  until the over ends, for up to 2 minutes, instead of cutting an FT8 or phone over off after 6 seconds.
- **Nothing else can key the radio while a voice-keyer message plays**, and Stop still ends it at once.

## FT8 and FT4

- **The full decode starts from the audio the early decode has cleared**, as in WSJT-X, so a weak reply on a busy band
  is found sooner, about 14.6 s into the period instead of 15.1 with 80 stations in it. A second search of the original
  audio keeps every station the old way found; on busy recordings it finds about 3 % more and none fewer. FT4 is
  unchanged. This needs a check on the air.
- **A call or Call CQ made during the other station's period goes out once.** Its over was keyed a second time about
  2 s later, behind the first. This needs a check on the air.
- **A logger's Replay gets back everything Band Activity holds**, oldest first, each with its period start and marked
  not new, as WSJT-X does.
- **A logger's free text goes into Tx 5, and out as Tx 5**, only while TX is on, as in WSJT-X, and a logger's clear of
  Rx Frequency clears only that pane.
- **The strip says so when the TX watchdog stops a call**, and the taskbar button flashes once.
- **Callsigns on the waterfall**, each over its own audio frequency, with the DX Call in red. Click one to move your RX
  offset there.
- **The Call Roster's Hide worked can reach back a set time** (Until 0000Z, 1 h, 4 h, 24 h or 7 days), for
  special-event stations on all week.
- **−B4 can hide only stations worked on this band**, so a station you need on a new band stays on screen.

## JTTY (needs a check on the air)

- **JTTY, WSJT-X's new keyboard mode, to receive and send**, under Digital beside PSK, with WSJT-X's frequencies,
  panes and waterfall, typed text, the F1–F8 contest messages and Live typing. Every send needs TX On and your licence
  across the whole signal, the TX watchdog runs, and Stop TX and Esc cut it at once.
- **JTTY is in the WSJT-X 3.3.0-beta4 test release only**, so the other station needs a WSJT-X 3.2 or 3.3 test
  release. It has not yet been tried on the air: a bench check should confirm keying, power and ALC at your data
  power, and that a WSJT-X station copies what Nexus sends.
- **JTTY contacts export as `MODE=MFSK SUBMODE=JTTY`**, as ADIF 3.1.8 registers it. LoTW does not list JTTY yet, so a
  LoTW upload holds JTTY contacts back and says so.
- **JTTY in Field Day and Winter Field Day**, and JTTY from a remote computer.

## Awards and alerts

- **More awards:** CQ WPX prefixes, Worked All Europe with band points, WWFF references with hunter levels, and Worked
  All Provinces.
- **The Fred Fish Memorial Award**, the 488 grids of the 48 contiguous states on 6 m, and a needed square on the Needed
  board says FFMA.
- **Custom awards** from a list of calls, prefixes, grids, references or ADIF field values, with band and mode filters.
- **A whole-log DXCC chart** and **What to QSL**, the unconfirmed contacts that would fill a slot an award still needs.
- **WPX and WAE chips on the Needed board**, off by default.
- **70 cm, 33 cm and 23 cm are full bands** for openings, need alerts and awards, and the 70 cm and 23 cm CW dials move
  out of the moonbounce segments.
- **The station decides every decode alert**, so the desktop and a computer driving it hear the same ones.

## Radios and the scope

- **An SDRplay receiver as the scope's source, through SDRconnect (needs a bench check).** With an RSP fed from the
  rig's RX OUT and SDRplay's free SDRconnect program running, the scope draws the receiver's whole span. Nexus only
  reads its IQ and sets its centre: it never transmits through it. It has been built and tested against the published
  SDRconnect API only, not yet with a real RSP, so treat it as untested on real hardware until a bench check confirms
  the IQ rate, where a carrier is drawn, retuning while the knob turns, and that RX OUT stays quiet while you transmit.
- **A FlexRadio panadapter updates twice as often (needs a bench check on a real Flex)**, 30 frames a second. A bench
  check should confirm it does not disturb a second radio sharing the connection.
- **The phone scope starts snappier**, and the rig scope refreshes about 30 times a second.
- **Disable a second radio without deleting it**, so another program can use its CAT port. A disabled radio leaves
  the radio switcher, and cannot be made active from Remote either.
- **FT-991, FT-710 and FTDX10: the ALC meter works again behind an older rigctld** (Hamlib before 4.6). This changes
  the CAT reads made while transmitting, so it needs a check on an FT-991 behind its own rigctld.

## Nexus Remote

- **One Devices list at the shack**, with Approve, Allow to transmit and Remove on each row. Remove takes a
  computer off every road at once when Nexus can confirm it is the same computer.
- **Allow to transmit, at the shack.** A device added by signing in is held from transmitting until you allow it or
  for its first 15 minutes, and the shack asks "Allow *Laptop* to transmit?". Only the station's own screen can press
  it. Approvals last across a restart.
- **Your own signed-in devices pair with your station automatically**, on your network and over the internet. A
  device signed in over the internet can transmit after its 15 minutes only once the email telling you it was added
  has been sent.
- **One Connect to my station… entry** on the computer you operate from.
- **Remote has its own Settings tab**, and the Remote page explains itself before you sign in.
- **Driving the shack from another computer:** the screen follows the shack's own mode, Phone's and CW's TX On/Off
  work, a refused press says why, Skip Tx1 is the shack's setting, and TX no longer turns itself off after a second or
  after every over.
- **A computer driving a remote station opens that station again next launch**, not its own.
- **Download my data or delete your account** from the Remote page.
- **Signing in and pairing again:** an unconfirmed email says so, and a station that no longer knows this computer
  offers Pair again.
- **Save to this computer saves only the station's own kinds of file**: a recording, an SSTV picture or ALL.TXT.

## Your screen

- **Pop-ups in the bottom-right corner** can be kept up for at least 10, 20, 30 or 60 seconds.
- **Tempo can come off the left-hand menu**, in Settings ▸ Appearance ▸ Features.
- **Spot from the phone cockpit and from every Spots row**, with the same review before it posts.
- **CW's Pitch box takes any pitch you type**, and the cockpit no longer comes back on your old pitch.
- **No empty window flashes on Windows** when Nexus starts maximised.

## Also in this release

- **Security: a computer no longer takes the station's internet key on Nexus Remote's word alone.** Linking a station
  paired with a code asks the station itself for its key over your network; away from home, or as an invited guest,
  the internet road carries nothing until you compare the station's key and press **It matches**. Your radio was never
  reachable this way: the station only ever lets in a computer you approved there.
- **Security: the station itself ends a press from a second computer after 3 minutes**, by its own clock, whatever the
  other computer does. **Still talking?** keeps Phone's Lock on with no break in the audio.
- **Security: a machine on your network can no longer keep your second computer out of Nexus Remote** by opening many
  slow connections.
- **Remote: a paired computer can no longer make the station hold hundreds of megabytes** in unfinished calls, and a
  flood of Stops no longer slows the station. A Stop is never lost or held back.
- **The FT8 and FT4 encoder test names the messages Nexus refuses to send**, so a mistake there fails the build.
  Nothing on the air changes.

## Known issues

- The SDRconnect scope source has not been run with a real RSP and SDRconnect yet.
- JTTY has not been tried on the air, and LoTW does not accept JTTY contacts yet.
- The Flex 30-frame panadapter and the FT-991 ALC fallback behind an older rigctld need a bench check.
- The FT8 decode change and the one-over fix for a call made during the other station's period need a check on the
  air.
- Activation files exported before this version are not known to the re-tag warning.
- Nexus Remote's internet road from a second PC still runs on a test service during the beta. When the regular
  service starts, the beta accounts and their stations are deleted, and each shack signs in or pairs again.

**118 changes since 1.20.0.** The full list is in the CHANGELOG.
