# Field Day

Nexus has a dedicated Field Day workspace that covers ARRL Field Day (June) and Winter Field Day (January) from a single settings switch, with dupe-checked all-mode logging, a live bonus checklist, real-time N3FJP TCP push and N1MM UDP broadcast, and Cabrillo 3.0 / ADIF export ready for ARRL submission.

---

## Event Switch and Date Rules

In **Settings → Contesting ▸ Contest**, choose between:

| Setting value | Event | Window |
|---|---|---|
| _(empty, default)_ | ARRL Field Day, the 4th full weekend of June | **27 hours**: 1800 UTC Saturday → 2100 UTC Sunday |
| `wfd` | Winter Field Day, the 4th full weekend of January | **30 hours**: 1600 UTC Saturday → 21:59 UTC Sunday |

A full weekend has both days in the month, so a Saturday whose Sunday falls in February does not count. Winter Field Day 2027 is **23 and 24 January**, the sponsor's own dates. Its rules said "the last full weekend" until 2025, and the two readings differ in a January with five full weekends: 2027 and 2028 are both such years, and in each the last full weekend is a week later than the event. If a sponsor moves a date, a rules update carries it (Settings → Contesting ▸ Contest ▸ Check for rules updates), and it applies at the next launch.

### Countdown

The banner at the top of the **Contest** screen (the tent item in the left bar) shows a live countdown as the event approaches:

- **starts in N days** / **starts tomorrow** / **starts in Nh** / **starting soon** / **active**

The header reads **active** once the window opens and clears back to countdown after the event window closes (27 hours for ARRL FD, 30 hours for WFD).

---

## Pre-Event Checklist

Run through this before the weekend. Most problems are discovered Saturday at 1759 UTC, not Friday evening.

- [ ] **Set your class and section** in Settings → Contesting ▸ Field Day (e.g. `3A`, `WI`). Both fields start **empty** — the greyed `1D` / `WI` you see are placeholder hints, not values. Contest mode will not engage until both are filled in (the exchange goes on the air), and an export with blanks is malformed.
- [ ] **Set power multiplier** (ARRL Field Day): x5 (QRP/battery), x2 (≤100 W, the default), or x1 (>100 W). The engine clamps illegal values to the nearest legal tier.
- [ ] **Configure N3FJP** (see [N3FJP Setup](#n3fjp-setup) below) and press **Test** to confirm the handshake before the event.
- [ ] **Configure N1MM address** if your club runs N1MM dashboards (see [N1MM Broadcast](#n1mm-broadcast)).
- [ ] **Verify CAT / PTT** on all bands you plan to use. Use **Settings → Radio ▸ Rig & CAT** and the Test Tone / key-up checks on [Rig and Audio Setup](Rig-and-Audio-Setup.md).
- [ ] **Test the Phone and CW cockpits** end-to-end: make a test QSO on a non-event day to confirm the FD log strip accepts a manual entry and that the dupe toast fires on a repeat.
- [ ] **Claim bonuses** (ARRL Field Day) or **tick objectives** (Winter Field Day) on the contest screen as you achieve them (see [Bonus Checklist](#bonus-checklist) and [Winter Field Day Scoring](#winter-field-day-scoring)).

### Winter Field Day 2027

The 2027 event is **23 and 24 January**, 1600 UTC Saturday to 21:59 UTC Sunday. Before it, on every laptop:

- [ ] **Get the 2027 rules.** Settings → Contesting ▸ Contest ▸ **Check for rules updates**, then restart Nexus: a downloaded rules file takes effect at the next launch. Until then a 1.14 to 1.17 copy counts down to 30 January, a week late. **Nexus 1.13 or older cannot read the current rules file at all**: upgrade, or its Winter Field Day starts a week late.
- [ ] Settings → Contesting ▸ Contest: **Winter Field Day**, then **Contest mode** on.
- [ ] **Class and category**: the number of transmitters you can run at once, then your category letter, `H` (home), `I` (indoor), `O` (outdoor) or `M` (mobile): `2O` is two transmitters outdoors. Then your ARRL/RAC section, `MX` in Mexico or `DX` anywhere else.
- [ ] **Power category** `LOW` (100 W PEP, the limit for everyone) or `QRP` (5 W on CW and digital, 10 W on phone, for the whole event). The Cabrillo file takes one of those two; HIGH writes no CATEGORY-POWER line.
- [ ] **Bands**: every amateur band **except 12, 17, 30 and 60 m**. Nexus does not warn on those four at Winter Field Day, so keep off them yourself.
- [ ] **Modes**: no WSJT mode (FT2, FST4, FT4, FT8, JT4, JT9, JT65, Q65, MSK144, WSPR, FST4W and Echo, the sponsor's list); the strip warns, and a contact in one of them counts for nothing. JTTY is not on that list. RTTY, PSK and SSTV are digital contacts.
- [ ] **Only direct contacts count**: one band, one mode, simplex, station to station. No cross-band, repeater, relayed, meshed or internet-linked contact counts; a satellite contact is logged and kept, and scores nothing (see [Winter Field Day Scoring](#winter-field-day-scoring)).
- [ ] **Spot and solicit only over amateur RF.** While the event runs Nexus posts no spot over the internet (see [Winter Field Day Spotting](#winter-field-day-spotting)).
- [ ] For a club: on each Windows laptop press **Allow Nexus through Windows Firewall** in Settings ▸ Contesting (club sync uses TCP 42073 and UDP 42074, the club TV scoreboard TCP 7373; the button allows them on Private and Public networks), and set **every clock from one source**, to the second. Nexus warns a position more than 30 seconds off the host's clock when it joins, and never sets a clock.
- [ ] Rehearse on an earlier day than the day before the event: entering the event loads only the contacts logged from 24 hours before it starts, keeps a rehearsal's out of its log by itself and deletes nothing (see [Contest Log Persistence](#contest-log-persistence)). A host leaves the rehearsal out of the club's log the same way, whatever its Event name.

During the event, tick each objective on the contest screen as you complete it. After it, export the Cabrillo or ADIF file and submit it as [Exports](#exports-cabrillo-adif-summary-and-dupe-sheet) describes, by **23:59 UTC on 1 March 2027**.

---

## Exchange and Mode Codes

ARRL FD exchange is **Class + ARRL Section** (e.g. `3A WI`). Winter Field Day's is a class number and a category letter, then the location: the transmitters you can run at once, `H`, `I`, `O` or `M`, then the ARRL/RAC section, `MX` or `DX` (e.g. `2O WI`).

Nexus logs three mode classes, matching ARRL's mode-class dupe rule:

| On-air mode | FD mode code | QSO points |
|---|---|---|
| FT8, FT4, TempoFast, TempoDeep | DIG | 2 |
| CW | CW | 2 |
| SSB / Phone | PH | 1 |

The same callsign counts **once per band per mode class**. Working K1ABC on 20 m CW and then 20 m FT8 are two legal contacts (different mode class). Working K1ABC on 20 m FT8 twice is a dupe — the log strip will reject the second attempt with an error toast. Dupe checks are case-insensitive.

---

## Scoring Formula

For ARRL Field Day the live scoreboard shows:

```
QSO points × power multiplier + claimed bonus points = total score
```

Score updates every snapshot cycle (approximately every 300 ms).

**Power multiplier tiers** (ARRL FD):

| Tier | Condition | Multiplier |
|---|---|---|
| QRP/battery | No commercial power | ×5 |
| Low power | ≤100 W | ×2 (default) |
| High power | >100 W | ×1 |

**Distinct section count** (the FD multiplier equivalent) is computed and displayed in the scoreboard. New sections receive a **Mult!** tag in the log table as they are worked.

### Winter Field Day Scoring

Winter Field Day scores by **objectives**: the sponsor's thirteen for 2027, each worth an objective multiplier (OM).

```
total score = QSO points × (OM + 1)
```

"The +1 is for participating", so a log with no objective completed still scores its QSO points. QSO points are counted as at ARRL Field Day (phone 1, CW and digital 2, each station once per band and mode); there is no power multiplier and no bonus list.

| Objective | OM |
|---|---|
| Operate station equipment on alternative power | 1 |
| Operate 100% on alternative Power | 2 |
| Operate away from home | 3 |
| Deploy multiple antennas | 1 |
| Send and receive the WFD SSTV image | 2 |
| Make at least three contacts on a cross-band repeater | 3 |
| Send and receive at least one Winlink email | 1 |
| Copy the Winter Field Day Special Bulletin | 1 |
| Make at least 3 contacts on at least 6 different bands | 6 |
| Make at least 3 contacts on at least 12 different bands | 6 |
| Use multiple modes | 2 |
| Operate the event QRP | 4 |
| Operate six continuous hours during the event | 2 |

The contest screen lists them where ARRL Field Day lists its bonuses, under **Objectives**. Tick each one as you complete it, or mark it **Plan**: only a ticked objective counts. Ticking 100% alternative power counts station equipment on alternative power with it, as the sponsor says it qualifies you for that one, and twelve bands counts six. For six bands, twelve bands and multiple modes the row shows what your log has (bands with three or more contacts, and modes worked); you tick them yourself. When you submit, the sponsor's form asks you to select your completed objectives again. If your Power category and the QRP objective disagree, the list says so.

The claimed total is the same number on the contest screen and its pop-out, the club line, the club TV scoreboard, the score summary and the Cabrillo `CLAIMED-SCORE`.

**A satellite contact counts for nothing**: the sponsor's rules say "Cross-band, repeated, relayed, meshed, and/or internet-linked contacts do not count." Nexus logs it and keeps it (it reaches your logbook and LoTW when you merge the contest log), scores it zero, leaves it out of the Winter Field Day Cabrillo and ADIF files, and does not count it as a dupe of the same station worked without the satellite.

### Winter Field Day Mode Rules

The WFD rules ban the entire WSJT-X mode suite (FT2, FST4, FT4, FT8, JT4, JT9, JT65, Q65, MSK144, WSPR, FST4W and Echo, the sponsor's own 2027 list) while explicitly keeping RTTY and SSTV legal as Digital. JTTY is not on that list. Nexus carries this list as rules data. It does **not** block or disable any mode: a contact in one of them is logged and exported like any other, and counts for nothing in the score Nexus shows and claims. Staying inside the rules is your call; what Nexus does guarantee is that a digital contact is exported and pushed under the mode actually used (an RTTY contact says RTTY, never FT8), so a legal contact can never be misreported as a banned one.

### Winter Field Day Spotting

The sponsor's 2027 rules: "You may spot yourself and others only via amateur RF." and "QSOs may be solicited only over amateur RF during the event." So while the event runs, a station with Winter Field Day switched on posts nothing over the internet: no PSK Reporter reports, no DX cluster spot from the Spot dialog, and no POTA self-spot, from the desktop or from Nexus Remote. The contest screen says so while it lasts, the Spot and Spot me buttons say why if you press them, and Settings says so beside PSK Reporter. Receiving cluster and skimmer spots carries on, your PSK Reporter setting is not changed, and everything posts again once the event ends. Asking for contacts anywhere but on the air is up to you to avoid.

---

## Scoreboard, Sections Board and Pop-Out

The Contest screen carries a live scoreboard: QSO and section counts, per-mode chips (DIG / CW / PH), and the score math for the active event (Winter Field Day shows its claimed total, QSO points × (OM + 1), never the ARRL power×+bonus formula).

- **Operator field** — Field Day rotates operators; type the call of whoever is at the key. It persists across restarts, and each QSO pushed to N3FJP is attributed to that operator (falling back to the station call when empty). While a contest runs it is at the top of the Contest screen, on the log line's title line beside Sending and I moved (and in the Logger window's); with none running it is here in the scoreboard, as it is in the torn-off scoreboard.
- **Sections board** — all 85 ARRL/RAC sections laid out division by division, each cell turning green with a ✓ as the section is worked, with a worked/total count. It doubles as your multiplier tracker. At a club it is the club's: a section any position works turns green on every laptop's board, the host's included, and this laptop's own are marked ⌂ in the colour its name has in the club grid, with a legend and both counts. A QSO party's county board works the same way.
- **Pop out** — the button in the scoreboard header tears the whole scoreboard (operator, tiles, sections board) off into its own window, sized for a second monitor or a club display facing the room. The docked view keeps working independently.
- **Club Board** — the **club band board** (position, band, mode, operator, QSOs, rate, clock) has its own button in the left rail, directly under Contest, and its own window. It appears whenever Field Day is on, whether or not club sync is running, and one click puts it on a second monitor: this is the board a multi-station club watches all event to see who is on what band before moving to another one. Above its table, a grid puts each position's name in the cell of its band and mode, the contest's bands across and its mode classes down, so a band and mode nobody is on is an empty cell; on the host each name is coloured by its link (green **Connected**, amber **Reconnecting**, red **Lost**, each with its mark, and a legend under the grid), a removed position is left out, and one with no band yet is named under the grid. The same **Pop out board** button in the club header on the dashboard opens the same window. It is set in larger type than the docked copy because it is watched from the operating position rather than read at the keyboard, and it is a monitoring window: no operator field and no export buttons, both of which live on the dashboard.
- **A full board** (on the host) — every club line each position is sent carries the whole board, and one line carries about 59 positions. From the moment one more might not fit, the host's club block says how many positions the club has; past that, each position's board leaves out the positions heard from least recently, and says how many it shows. Contacts, dupe warnings and the score still sync. The host's own board still lists every position that has joined, each with its Status and its Remove.
- **Status** (on the host's board) — each position's line says where its link stands, with when the host last heard it: **Connected** while the host has heard it in the last 10 seconds (each position sends a heartbeat every 5), **Reconnecting** once it has missed its heartbeats or its link has just closed, and **Lost** once 15 seconds pass without a word from it, when the host closes its link (the same 15 seconds after which either end gives up a silent link); "heard 12 s ago", "heard 4 min ago" beside each. The host's own line reads *This laptop*. The host's "other positions connected" line counts the lines that read Connected, so the two always agree. A position the host has not heard since it started (after a restart of the host) reads *not heard since this Nexus started* until it rejoins. Each joined laptop's own club line says the same of its link to the host: Connected, with when it last heard the host, Reconnecting, or Lost. The words are on the host's own screen and in the club board pop-out, never on the spectator scoreboard or Remote.
- **Remove and Allow back** (on the host's Contest screen) — every other position's line on the host's club board has **Remove**. A press asks first, naming the position and saying that its contacts stay; **Remove it** closes that laptop's link, and the host turns it away each time it tries again, its line reading Removed, until you press **Allow back** on that line. The removed laptop's own screen shows the host's sentence: the host removed this laptop's position, its contacts stay in the club's log, and the ones it logs meanwhile go up if the host lets it back in. Nothing it already sent changes: the club's log, its dupe warnings, the score, the club Cabrillo and ADIF are what they were. After **Allow back** it rejoins on its next try, within 15 seconds, and sends what it logged while it was out, as a laptop back from an outage does. A removal is kept through a restart of the host. The host is refused by position id, which only that laptop's settings carry: renaming the laptop's position does not get it back in; a laptop whose settings are reset to a new position id joins as a new position, with a contest log that starts empty, and its line shows up as that, where you can remove it too. Remove is neither Give (which hands a position to another laptop) nor Ctrl+D (which takes back one contact), and both work as before. The host's own position cannot be removed, and Remote has neither button. Remove transmits nothing and touches no radio.
- **Turned away** (on the host) — a position the host refuses when it joins (another contest, an older Nexus than the host's, at a QSO party a laptop set up on the other side of the state line, in any contest but ARRL Field Day a laptop on another callsign than the club's, in any contest a laptop naming a club position another laptop already holds at this event, or one whose Callsign on the air is not a call sign, or whose club position id Nexus did not make) is named on the host's club block with the sentence that position was sent, which its own club block shows too. The club's callsign is the host's, and it stays the club's after **Contest over?** puts the host's own callsign back: a laptop still on the club's callsign is let in and sends the contacts it still owes. A laptop with the same club key as one connected as its position now (one laptop's whole settings folder copied to another) is turned away by name while the first is connected, and told how to give it a position of its own; a laptop back from a dropped link can meet the same words for up to about 30 seconds, until the host notices its old link is gone. A laptop turned away because another laptop holds its position has **Give this laptop its position** on its line in the club block on the host's Contest screen. A press first shows that laptop's club code: look at that laptop's own Contest screen, and press **The codes match: give it the position** only if the code beside Club there is the same. The host then gives it the position, and it joins on its next try and sends the club its whole log, while the laptop that held the position is closed and turned away by name, and the contacts it sent as that position leave the club's file until it rejoins with a position of its own. That laptop's line then has the same button, which gives the position back. Only the host's own screen has the button; Remote never does.
- **Another callsign** — the club's log takes a contact only on the club's callsign. A contact logged on another (on the host after **Contest over?**, with Contest mode turned back on under its own callsign to reach **Club Cabrillo**, or on a laptop whose Callsign on the air was changed while it was joined) stays in that laptop's own log and is never sent: its club block says how many, and the club chip does not count them, so it still reaches Synced. Nothing already in the club's log changes. ARRL Field Day is the exception, as at the join: its GOTA station's contacts go to the club under the GOTA callsign.
- **Club code** — each laptop's club block shows its own club code (two groups of four letters and digits, like `7KQ2-M9XD`), made from the club key that laptop made for itself. The host shows the code of each laptop it turned away beside the name and call that laptop gave, which any laptop can give, so the code is what tells two laptops apart. Codes are only on that laptop's screen and the host's: never on the spectator scoreboard, on Remote or on the club board.
- **Two positions with one name** read apart on every board, the spectator scoreboard and Remote: the one the host heard of first keeps the name, and the next is shown with (2) after it.
- **Kept out** (on the host) — a contact the host keeps out of the club's log (one whose call is not a call sign, or that holds something the club's file cannot carry) is listed on the host's club block for the rest of the event, through a restart of the host: how many there are, and the newest 16, each with the laptop that sent it and why. It stays in that laptop's own log; export that laptop's own Cabrillo to add it to the club's file by hand.
- **Another county** (on the host) — a laptop set up on another county than the host's (or another state, province or section) joins, and every contact it logs sends that county under the club's call. Once it has logged one, the host's club block names it, with what it sends and what the club sends ("CW tent sends KANE, and this club sends MCLN"), until one of its contacts sends the club's again. Its contacts are taken as sent: set its County, or press **I moved** on it. Only the host's own screen shows this.
- **Club log** (on the host) — under the club board, on the host's Contest screen, in its pop-out, in the Logger window and in the Club Board window: the contacts the club's log holds, newest first by the time each was logged (on the clock of the laptop that logged it), with the time, call, exchange, band and mode as your own log shows them and the position that logged each, named as on the board. A repeat of a contact another position made first is marked the way your own log marks a dupe: at Field Day the club's files leave it out, and in a contest that wants repeats reported they keep it as a zero-point line. These are the contacts **Club Cabrillo** and **Club ADIF** are written from, read the same way, so the list and the files agree, and a removed position's contacts stay on it as they stay in the files. It lists the newest 100 and says how many the club holds, so a long contest does not slow the screen; every contact is in the files. A joined laptop holds no merged log, so it has no club log, and the Remote page does not carry it.
- **Clock** (on the host's board) — each position's clock against the host's, which the position measures every 5 s over the club link and reports to the host: *in step* under 2 s, how many seconds ahead or behind from there, in the warning colour past 30 s, and a dash for a position running an older Nexus. Each position's own club line says the same from 2 s ("This PC's clock is 3 s behind the host's") and warns past 30 s. Nexus only shows the difference and never changes a clock, so FT8/FT4 timing is untouched; put a wrong clock right in that PC's date and time settings.
- **With club sync off**, the window says so and names the route that turns it on (Settings ▸ Contesting ▸ Field Day Club Sync ▸ Host a club event) instead of showing an empty board. With sync on and nobody else logging yet, it says it is waiting.
- **Spectator scoreboard** — a web page for a TV or projector facing the room (Settings ▸ Contesting ▸ Field Day Club Sync ▸ **Spectator scoreboard**; the row shows the address to open on the TV). It reads across a room at 1080p and at 4K: the claimed score and how it is made, the rate, each position's band and mode, the contacts by band and mode, the latest contact, the time left, and a map of what the contest counts — the sections globe for both Field Days, the 102 counties for the Illinois QSO Party. It names the club's callsign, which stays the club's after **Contest over?** puts the host's own callsign back. It works for **any contest club sync runs**. The host shows its own club; a **position shows the host's board**, so the TV can sit at any table, as long as the host's Spectator scoreboard is on too, on the same port. If the host can't be reached the TV says so in plain words, keeps the last board, and comes back by itself; a station with no club says so, with what to do. Add `?theme=light` to the address for the light board, or `?theme=auto` to follow the TV.

---

## Bonus Checklist

The bonus checklist contains ARRL Field Day's 16 bonuses. Toggle each one on the Contest screen as your club achieves it:

| Bonus | Points |
|---|---|
| Emergency power | 100 |
| Media publicity | 100 |
| Public location | 100 |
| Public info table | 100 |
| NTS message | 100 |
| W1AW bulletin | 100 |
| Natural power | 100 |
| Elected official visit | 100 |
| Agency visit | 100 |
| GOTA | 100 |
| Youth | 100 |
| Safety officer | 100 |
| Social media | 100 |
| Educational activity | 100 |
| Satellite QSO | 100 |
| Web submission | 50 |

**Total possible bonus: 1 550 points.**

The bonus checklist is ARRL Field Day's. Winter Field Day has no bonus list: it scores by its objectives (see [Winter Field Day Scoring](#winter-field-day-scoring)).

---

## All-Mode Logging

### Digital (FT8, FT4 and TempoFast auto-sequencer)

When the FD workspace is open and a digital contact is in progress, the auto-sequencer handles the exchange once you start it:

- **S&P** (Search-and-Pounce): double-click a CQ decode → sequencer sends your exchange → accepts their roger → sends RR73 and logs the QSO. On FT8 and FT4 this works from the FT cockpit too, and so does a reply to a CQ from JTAlert or GridTracker: your exchange goes out in the period opposite theirs, and afterwards you are in the contest's S&P. In ARRL Field Day and Winter Field Day on FT8, FT4, FT2, FT1 and DX1, a double-click or a logger's reply answers with your class and section and logs in the contest while you are in the contest's S&P; on FST4, Q65, MSK144 and JT65 it works the station as a normal QSO, as before. With **Disable TX after sending 73** on (it is by default), TX goes off once that contact's last over has gone out, as WSJT-X does after a double-clicked contact. The contest's S&P never calls a station by itself, with TX on or off: every contact starts with your double-click or a logger's reply, and afterwards S&P listens until you pick the next CQ. A double-click on a station that is calling you, and a click on a roster row, still work the station outside the contest.
- **Running**: answer an incoming exchange → roger with your exchange → accept their RR73 → log. On FT8 and FT4 the run then sends 73, as WSJT-X does, and calls CQ FD again. On TempoFast it goes straight back to CQ FD.
- **Call CQ** on the FT cockpit, with Contest mode on, is the contest's Running (`CQ FD`), the same as the Running button on the Contest screen. If your class or section is not filled in, it tells you so and calls nothing. The Tx6 row still sends exactly the CQ it shows, outside the contest.
- **S&P** on the FT cockpit, with Contest mode on, is the contest's S&P, the same as the S&P button on the Contest screen: it listens, and you double-click a CQ to call that station. If your class or section is not filled in, it tells you so and changes nothing.

Opening **Contest** in the left bar with Contest mode on, coming from another mode, starts the contest in **Search-and-Pounce**; it never starts calling CQ. If you are already in the contest, running or in S&P, it opens the screen and leaves you as you were: a run keeps calling CQ while you check the score, and a contact in progress carries on. With the mode off it opens the screen and nothing else. Switch to Running via the button pair in the Contest screen's header.

While Contest mode is on, the WSJT-X UDP `Status` message carries the special operation WSJT-X reports for the contest that is running: `special_op = 3` (Field Day) in ARRL Field Day, 1 (NA VHF) in the ARRL VHF contests, and 0 in the others. Winter Field Day is 0, because WSJT-X has no Winter Field Day mode. Once the **WSJT-X UDP API** switch in Settings → Logging & Connectors ▸ Integrations & Feeds is on (it is off by default), JTAlert and GridTracker activate their FD-specific behavior during ARRL Field Day without any other configuration on your end. FD contacts are also emitted as `QsoLogged` UDP datagrams to the same sink.

### CW

Navigate to the CW cockpit. The log strip detects that Contest mode is on and shows **Class** and **Section** fields alongside the standard call/RST fields. Fill in the exchange and press Log — the entry routes to `fdLogManual()` with mode code `CW` and is dupe-checked against the FD log.

The CW cockpit pre-fills Class from the most recent FD entry so you do not retype it for every contact.

### Phone

Navigate to the Phone cockpit. The log strip similarly adds Class and Section fields and routes to `fdLogManual()` with mode code `PH` (1 point). RST defaults to 59.

All three mode classes write into the **same unified FD log**, so the live score and Cabrillo export reflect the full multi-mode total in real time.

### The Contest screen

With a contest running, the **Contest** screen has a log line of its own at the top: the logger window's large Call and exchange boxes, with the dupe warning right under them, then your last five contacts, newest first, and the rest of the screen below, scrolling on its own. **A−** and **A+** make the boxes and the last contacts smaller or larger, from 80 to 160 %, on top of your Text size setting; this computer remembers the size for the main window, and the logger window keeps its own.

It is the same log line as the cockpits' LOG panes: the same boxes, dupe warning, Super Check Partial line, call history and keys (Ctrl+D, Ctrl+Y, Alt+W, Ctrl+O), and with the logger window open the same contact as that window, so a contact logs once. It logs under the mode of the last cockpit you used, shown with the band in its top row: phone as PH, CW as CW, RTTY, PSK and the FT modes as DIG. If that mode is one the contest does not permit (FT8 or FT4 at the Illinois QSO Party), a small **FT8 · no credit** (or **FT4 · no credit**) chip beside the mode says so, and hovering it shows the whole sentence; the contact still logs and goes in the files, and it counts for nothing in your score, as the sponsor will not count it. The screen's banner and the FT screen's header show the same chip, and only while you are on such a mode.

Nothing on it transmits: no PTT, no F-key messages, no Enter Sends Message and no Tune. **Esc** there stops TX, as the top bar's **Stop TX** does, and leaves the boxes as they are. With Contest mode off, or no contest that can start, the screen is as it was.

### Contest Log Persistence

The contest log survives restarts: every logged contact is journaled to `fieldday_backup_<id>.adi` beside `settings.json`, where `<id>` is the eight-character code this computer gave itself the first time Nexus ran (for example `fieldday_backup_a1b2c3d4.adi`), and the journal is restored automatically whenever Contest mode puts the station back in the contest — a mid-event quit, crash, or Run/Search-and-Pounce switch loses nothing. Every contact of the running you are in comes back, however long ago it was logged, so a log exported a week or a month after the contest is complete. An earlier event's contacts are not restored.

Only the session's own contacts are restored: the contest you are entering, logged from 24 hours before this running of it starts. A rehearsal on an earlier day, or another contest worked in the last four days, stays in the journal untouched and out of this log, and the Contest screen says how many contacts it kept out and why. A gear test within 24 hours of the start is part of the event's log; to start clean after one, click **Clear test contacts** on the Contest screen twice (see Take Back the Last Contact, below). The test contacts leave the log and wait under **Removed**, and in a contest that sends a serial the next contact is 001, after a restart too.

### Take Back the Last Contact

Logged a contact you should not have, or stopped your TU after the contact was already in the log? Press **Ctrl+D** from anywhere in a cockpit that shows the contest log strip, or click **Remove last** beside Clear. The first press names your newest contest contact on the strip's message line, for example `Remove K9AAA · 20m CW · 17:32 · 599 COOK? Press Ctrl+D again.` A second press within 5 seconds removes it. Any other key, the 5 seconds running out, or a new contact cancels it, and if the newest contact changed between the two presses nothing is removed and the strip says so. A held-down Ctrl+D counts as one press. The caret stays where it was, and Ctrl+D never keys or stops the transmitter: Esc still stops TX. A contact on a county line goes with all its counties. On the contest screen the newest row of the log has its own **Remove**, which asks the same way: click it twice.

Nothing is deleted. The contact leaves the QSO count, the score, the multipliers, the rate, the dupe check, the Cabrillo, the contest ADIF, the summary and a later merge into your logbook, and the contest screen keeps it under **Removed**. **Restore** there puts it back exactly as it was, with the same time, serial number and club-sync number, unless you have worked that station again on that band and mode since; then it stays removed and the screen says why. A removed contact's serial number and club-sync number are never handed to another contact, not even after a restart, because the journal keeps the removed contact, marked as removed.

The contact may already have gone elsewhere, and Nexus cannot take it back from there. The strip says where: N3FJP, the N1MM broadcast and WSJT-X listeners, once a slot boundary had sent it on to them, and your logbook if you had merged the contest, with the services that copy was uploaded to. Delete it in those places yourself if you need to. Removing it from the contest log never touches your logbook. The other way round does work: deleting a contact in the Logbook that you merged from the contest still running also removes it from the contest log (it appears under Removed, and Restore brings it back), the way an edit of its call or band in the Logbook already corrects the contest log.

**At a club the club takes it back too**, at a position or at the host: the host keeps the contact marked taken back, never deleted, every laptop's club dupe check and the club board drop it, and the club Cabrillo and the club's counts leave it out. **Restore** brings it back everywhere the same way. The strip says when: as soon as the host has it while this laptop is with the host, or once the link is back, and the club chip counts the take-back until then. Each laptop takes back only contacts it logged itself, the host's own included: a contact another position logged is taken back on that laptop, or, if that laptop is gone, in the club's Cabrillo file before you send it in. Every laptop in the club must run the same Nexus release, because an older one cannot take a contact back: the host turns it away when it joins and says to update it. Removal is not offered on the hosted Remote page or in the Remote client.

**Clear test contacts**, beside the exports on the Contest screen, takes every contact of this running out of the contest log at once, for the contacts you logged as a test before the start. The first click names how many, and a second click within 5 seconds clears them; a contact logged in between cancels it, and nothing is cleared. Each goes as Remove takes one: it waits under **Removed**, where **Restore** puts it back, it leaves the score, the dupe check and every export, and its club-sync number is never handed to another contact. Unlike Remove, Clear test contacts starts the serial numbers again: in a contest that sends a serial, the next contact is 001, after a restart too, and only in such a contest do the button's question, its tooltip and the notice after it say so. A contact you restore afterwards keeps the number it was sent and does not move the numbers on, so a later contact can send that number again. At a club only this laptop's contacts go, and the club drops them on every laptop as it drops a removed one. Clear test contacts is not offered on the hosted Remote page or in the Remote client.

Remember that most sponsors check logs against each other: if the other station logged you, a contact you remove becomes a not-in-log for them. When in doubt, keep it.

### Edit a Contact

A busted call, a mistyped exchange, or a contact logged on the wrong band or in the wrong mode is fixed where it stands, without taking it back. Press **Ctrl+Y** from anywhere in a cockpit that shows the contest log strip: your newest contact opens for edit in place of the strip's boxes, with the caret in its call. On the contest screen every row of the log has **Edit**, which opens that contact in its row. Change the call, the exchange they sent, the band or the mode, then press **Enter** or click **Save**; **Esc** or **Cancel** leaves it as it was. A contact on a county line is edited as one contact: its counties are in one box, joined by `/`, and an edit keeps the same number of counties.

The contact keeps its time, its serial number and its club-sync number, so an edit never uses up a number and the next contact is numbered as it would have been. The edit is in the journal before the screen answers, so a restart keeps it. The dupe marks, the QSO count, the multipliers and the score follow at once, for the edited contact and for every contact after it: a busted call corrected to a station you had already worked becomes the dupe it was, and a dupe corrected to the station it really was counts again. The Cabrillo, the contest ADIF and a merge into your logbook made after the edit carry the contact as edited. A new band clears the frequency the contact was logged on, which belonged to the old band, so the files give the band. Each box is checked as the strip checks it, so an edit cannot save an exchange the strip would not log.

The contact may already have gone elsewhere, and keeps what it said there: the screen names N3FJP, the N1MM broadcast and WSJT-X listeners if a slot boundary had already sent it on, and your logbook if you had merged it before the edit. Fix it in those places yourself; in the Logbook, an edit of its call or band also corrects the contest log. Enter in the edit only saves the edit, and sends nothing even with Enter Sends Message on. Ctrl+Y never keys or stops the transmitter: Esc still stops TX.

**At a club an edit is not offered yet**, at the host or at a position: club sync cannot carry an edit to the other laptops, and an edit made on one laptop only would leave the club's Cabrillo with the busted call. Take your newest contact back instead (Ctrl+D twice, or **Remove** on the contest screen) and log it again: the club drops it on every laptop. An older contact logged at a club is fixed in the club's Cabrillo file before you send it in. Editing is not offered on the hosted Remote page or in the Remote client.

### Wipe the Line, and the Operator Box

**Alt+W** clears the whole log line, the call and every exchange box (a signal report goes back to 59 or 599), from anywhere in a cockpit that shows the contest log strip. It releases the serial number in flight, and Enter Sends Message starts a new contact. Press it again right after, with no other key between, and it puts back what it cleared, call-history marks included, and the station gets its serial number back. On a Mac it is Option+W. **Clear** keeps Field Day's class and section in their boxes; Alt+W clears those too.

**Ctrl+O** takes you to the operator box from the contest log strip of any cockpit: the Contest screen opens with the cursor in its **Operator** box, where whoever takes the seat types their call and presses Enter. In the logger window the cursor goes to that window's own operator box. Only the screen changes: Contest mode, the FT8/FT4 role and the radio stay as they are. In Phone, leaving the screen stops a message the voice keyer is playing and discards a recording in progress, as any change of screen does, and says so.

Neither key transmits or stops a transmission, and neither takes Esc or Space.

---

## Enter Sends Message (ESM)

With **Enter Sends Message** on, each press of Enter in the contest log strip sends the contact's next message from your F-keys, and the press for the contact's last step logs it. It works in the CW, RTTY and Phone cockpits while a contest runs. It is **off** until you turn it on, separately for each cockpit, in [Settings → Contesting ▸ Enter Sends Message](../guide/settings-reference.md#enter-sends-message) or with the ESM switch in the cockpit's TX dock.

### How a contact runs

ESM starts in search and pounce (S&P). Pressing the key that holds your CQ switches to running, and clicking a spot switches back to S&P.

| You are | The strip holds | Enter sends |
|---|---|---|
| Running | no call | your CQ |
| Running | a call, and your exchange has not gone to it | his call and your exchange; if his exchange is not complete yet, the cursor moves to his first exchange box |
| Running | your exchange has gone, his is not complete | AGN |
| Running | your exchange has gone, his is complete | TU, and the contact logs |
| Running | a dupe of your own log, and your exchange has not gone to it | QSO B4, once; the cursor stays |
| Running | a dupe QSO B4 has gone to, and his exchange is complete | nothing; the contact logs, marked a dupe, for 0 points |
| S&P | no call, or a call whose exchange is not complete | your call |
| S&P | a complete exchange | your exchange, and the contact logs |
| S&P | a dupe of your own log | nothing, and nothing logs |

**A dupe calls you while you run.** Enter sends QSO B4 once (**F7** in the CW and RTTY contest sets), and the next Enter logs the contact, marked a dupe and worth 0 points, without sending anything. If his exchange is not copied yet, that Enter sends nothing and logs nothing and the strip says so: copy it, or press Alt+W to clear the line. If your exchange had already gone to him, the contact goes on as any other, AGN or TU and the log; in a QSO party a dupe shows only once his county is in the box, by which time you have usually sent your exchange. In S&P Enter sends nothing to a dupe: you do not call a station you have already worked. The **Log** button and Alt+Enter log a dupe at any time, sending nothing.

In S&P, when call history completes his exchange as you type his call, the first Enter still sends your call, and the next sends your exchange and logs the contact.

With **Call once (S&P)** on, your call goes once per station: the next Enter sends AGN instead. If you correct a busted call after your exchange went out, the corrected call gets your exchange again. In Phone, when you are running, Enter plays nothing for his call and your exchange: you say them yourself, because a recording cannot say a callsign, and Enter moves the cursor to his exchange.

His exchange counts as complete only with what you gave for this contact: what you typed or picked, the 5NN or 59 nobody changed, or a box filled from call history (marked **history**). A box filled from call history counts as copied: Enter sends and logs it as it stands, so type over it when the station sends something different. In Field Day, the class and section the strip keeps from your last contact count only once you type them for this one.

**In the TX dock**, beside the ESM switch, the plate shows **Run** or **S&P** (click it to switch) and what the next Enter does: the key or keys it sends, which glow on the dock, or why it sends nothing, or why ESM steps aside. Its tooltip reminds you that Alt+Enter logs without sending anything.

### The rules

1. ESM is off until you turn it on, separately in the CW, RTTY and Phone cockpits. Nothing turns it on for you, and it remembers your choice.
2. It works only in the contest log strip of those three cockpits while a contest runs. It does nothing in FT8, FT4 or any FT mode, nor in Digital, Tempo, PSK, SSTV or APRS, nor on the hosted Remote page.
3. Enter sends the message of the next step: CQ, his call and your exchange, TU, your call, your S&P exchange, AGN, or QSO B4. The message comes from your own keys where you mapped them to that step, and otherwise from the built-in set's layout, and it goes out by the same path, with the same checks, as that F-key would send it.
   - If your set has no message for the step, Enter sends nothing and logs nothing, and the strip names the step.
   - If your set has no step mapped at all, ESM steps aside: Enter logs as it does with ESM off.
4. Enter sends only when TX is already on, the dial is inside your license privileges, and no clock repair is running. In Phone it also needs you not to be holding PTT, the keyer not to be recording, and the radio not to have the mic. Otherwise Enter sends nothing and logs nothing, and the strip says why.
   - While the RTTY auto sequence runs or Continuous TX is latched, or while the Phone voice keyer is hidden, ESM steps aside: Enter logs as it does with ESM off, and says why.
5. Enter never turns TX on. After Stop TX, Esc or the watchdog, Enter is refused until you turn TX back on yourself: in CW by sending with an F-key, in RTTY with the TX button, in Phone with PTT. Going to another screen and back does not turn it on either.
6. One press, one message. Holding Enter sends once. Alt+Enter logs without sending anything.
7. The call it sends is the call in the strip.
8. The contact logs the moment you press Enter for its last step: TU when you are running, your exchange in S&P, and for a dupe that called your run, the press after its QSO B4. It logs only a complete exchange, and only once that last message has started. An Enter that is refused logs nothing.
   - If you then stop that message, with Esc, Stop TX or the watchdog, the contact stays logged and the strip has already cleared for the next one. The strip says so. To take the contact back, press Ctrl+D twice: it leaves your contest log, score and exports, and you can restore it from the contest screen. At a club the club log drops it too.
   - If you stop an earlier message of the contact (his call and your exchange, or your call), it counts as not sent, and the next Enter sends it again rather than logging.
9. On a dupe of your own log, Enter sends QSO B4 once when you are running and your exchange has not gone to him, and the next Enter logs the dupe; in S&P it sends nothing and logs nothing. A club dupe (another position worked them) stays a warning, and Enter goes on as it would for a new call.
10. An SCP (Super Check Partial) match is never sent and never logged until you accept it. A box filled from call history counts as copied: Enter sends and logs it as it stands, so type over it when the station sends something different.
11. Esc and Stop TX stop it at any step, from anywhere in the cockpit.
12. Nothing is sent by a timer. Every message is one press of yours.

### Which key sends each step

The built-in sets use the standard contest F-key layout: **F1** CQ · **F2** his call and your exchange · **F3** TU · **F4** your call · **F6** your S&P exchange · **F7** QSO B4, to a dupe that calls you while you run · **F8** AGN. **F5** (his call alone) stays a key you press yourself. RTTY's Contest set uses this layout, and so do CW's contest sets and its Field Day set; the [CW manual](CW.md#eight-f-key-macros) lists their texts. RTTY's Everyday set and CW's everyday set have no steps.

The voice keyer's slots follow the same layout: **F1** CQ · **F2** your exchange · **F3** TU · **F4** your call · **F8** AGN. No slot holds his call and your exchange, because you say those yourself when you run. **F5**, his call alone on CW, and **F6** stay yours, because a recording cannot say a callsign. A slot with no recording plays nothing, and the strip names the slot to record. AGN used to be on **F5**: if you ran ESM in Phone with AGN there, **F5** keeps whatever you recorded and the update gives **F8** a copy of it, so every slot plays what it played before. The keyer has no F7, and no slot holds QSO B4: until you record one in a slot of your own (**F5** or **F6**) and map QSO B4 to it in Settings → Contesting ▸ Enter Sends Message, Enter plays nothing for a dupe and the strip names the step.

**Your own keys.** Settings → Contesting ▸ Enter Sends Message lists the steps for each cockpit: the active CW profile, each RTTY set, and the voice keyer. For each step, pick the key that sends it, or two keys that go out as one message (F5 then F2, for a set that sends his call and then the exchange). Phone takes one slot per step. A step you leave on *Built in* uses the built-in key, and each row shows what Enter would send there, or why it would send nothing. The mapping is saved with its set: a CW profile carries its own, and so does each RTTY set. A CW profile still on the built-in sets shows their steps and has nothing to map until you customize it in Settings → CW.

---

## Band Follows QSY

When you change frequency — whether via a software dial command or by turning the rig's VFO knob — the active FD log's band field updates immediately. You do not need to manually change a "current band" setting mid-event. Without this, a QSY between bands would stamp subsequent contacts under the wrong band in Cabrillo, corrupting dupe keys and the band-column breakdown.

---

## N3FJP Setup

N3FJP Field Day Contest Log is widely used by clubs as the master log. Nexus pushes each new contest contact to N3FJP over TCP after logging it. By default it fills in N3FJP's entry form and presses its Enter (the ENTER sequence, which scores the contact); with **Use ENTER for contest scoring** off it inserts the contact with `ADDDIRECT` followed by `CHECKLOG` to refresh the N3FJP screen. The ENTER sequence fills the boxes N3FJP documents for the program the contest is logged in: class and section at Field Day and Winter Field Day; both reports and the serial received in CQ WPX, where N3FJP is also told the serial you sent so the two logs number the contact alike; both reports, the zone and the state or province in CQ WW RTTY; the grid in the VHF Contest Log (ARRL January, June and September VHF).

**In N3FJP first:**

1. Open N3FJP Field Day Contest Log.
2. Go to **Settings > Application Program Interface**.
3. Enable the API and confirm the port (default **1100**).
4. Leave N3FJP running and reachable on the LAN.

**In Nexus:**

1. Open **Settings → Logging & Connectors ▸ N3FJP Integration**.
2. Enter the N3FJP host (e.g. `192.168.1.50` or `localhost` if co-located).
3. Leave the port at **1100** unless you changed it in N3FJP.
4. Press **Test**. A successful test returns the program name and version string (e.g. `N3FJP Field Day Contest Log v6.6`). The button is disabled when the host field is blank.

The push runs in a spawned thread, so a slow or unresponsive N3FJP host never stalls the slot loop. Connection and read/write timeouts are each 4 seconds. Push errors are logged to stderr (visible in the Nexus developer console); they are not surfaced in the UI beyond the initial Test button. N3FJP push is disabled when `n3fjp_host` is empty.

---

## N1MM Broadcast

Nexus emits a `<contactinfo>` XML UDP datagram for each new FD QSO, compatible with N1MM+ network dashboards. Each datagram includes: mycall, call, band, mode, UTC timestamp, section, QSO points, contest name, rxfreq/txfreq (in units of 10 Hz), sent exchange, and a 32-hex per-QSO dedup ID.

**Setup:**

1. In **Settings → Logging & Connectors ▸ N1MM+ Integration**, enter the broadcast target, e.g. `192.168.1.255` or `192.168.1.50`.
2. If you omit the port, Nexus defaults to **port 12060** (the N1MM+ contactinfo default).
3. Broadcast is disabled when the address field is empty.

N1MM broadcast is **UDP emit-only**. Nexus does not receive or aggregate inbound `<contactinfo>` datagrams from other stations on your network.

---

## Exports: Cabrillo, ADIF, Summary and Dupe Sheet

All four exports are available at any time during or after the event from the Contest screen's export buttons.

### Cabrillo 3.0

- Each QSO line carries a real `yyyy-mm-dd hhmm` UTC timestamp derived from the logged Unix timestamp.
- Mode tokens follow Cabrillo 3.0: `CW`, `PH`, `RY` for RTTY contacts, `DG` for other (or unrecorded) digital.
- `CONTEST:` header is `ARRL-FD` or `WFD` based on the event switch. These are Cabrillo names,
  from the WA7BNM Master List of Cabrillo Names that the Cabrillo V3 specification points to;
  they are not always the same string as the ADIF `CONTEST_ID` below (ARRL Field Day's ADIF id
  is `ARRL-FIELD-DAY`).
- `CATEGORY-OPERATOR` is the **Entry category** you pick in Settings → Contesting ▸ Your entry (`SINGLE-OP` by default).
- **The rest of the entry's declarations** are set in the same place, and Settings shows each only for a contest whose sponsor's Cabrillo template has it. Each is written only once you set it, so a log you declare none of them for exports exactly the file it did before (in the CQ contests, with the category lines the CQ sections below describe):
  - **Multi-operator class**, for a `MULTI-OP` entry: `CATEGORY-TRANSMITTER`, in the classes the sponsor scores — `ONE`, `TWO` or `UNLIMITED` (Multi-Single or Multi-One, Multi-Two, Multi-Multi) for CQ WW, CQ WPX and CQ WW RTTY; `ONE` for Sweepstakes, whose only multi-operator category is single-transmitter; `ONE`, `LIMITED` or `UNLIMITED` for the ARRL VHF contests; `ONE` or `UNLIMITED` for the New York QSO Party. The `OPERATORS` line comes with it, because these sponsors require the operators of a multi-operator entry (CQ's robot rejects a log without them). Nexus marks every contact as transmitter 0, so add the second transmitter's contacts to a Multi-Two log before you send it. CQ's Multi-Distributed category is not offered: it needs a `CATEGORY-STATION` line Nexus does not write.
  - **Band category**: `CATEGORY-BAND` — `ALL`, or the one band of a single-band entry, in the bands the sponsor lists: `160M` to `10M` for CQ WW and CQ WPX, `80M` to `10M` for CQ WW RTTY. A single-band entry still logs every contact, and the sponsor scores only the band named. CQ WW RTTY writes the band its log holds when this is not set, as it always did.
  - **Overlay**: `CATEGORY-OVERLAY` — `CLASSIC`, `ROOKIE` or `YOUTH` for CQ WW, the same plus `TB-WIRES` for CQ WPX, `CLASSIC` or `ROOKIE` for CQ WW RTTY, and `ROOKIE`, `YOUTH12`, `YOUTH17` or `YL` for the New York QSO Party. CQ's overlays are for single operators, and for Rookie and Youth CQ asks for the date you were first licensed or your birth year in the soapbox. Sweepstakes' Limited Antennas Overlay has no Cabrillo value its sponsor names, so add it on ARRL's upload page if you enter it.
  - **Mailing address**: the street lines, then the city, state or province, postal code and country, in the `ADDRESS` lines of the CQ contests, the New York, Illinois and Tennessee QSO Parties, Sweepstakes and the ARRL VHF contests. Where the sponsor's template has only `ADDRESS` lines (ARRL's, Tennessee), the city, state and postal code go on one more of them, `Newington, CT 06111` style, as ARRL's own template writes them.
  - **Soapbox**: your comments, one `SOAPBOX` line per line you type (a long one is wrapped to Cabrillo's 75 characters), for the CQ contests, the ARRL VHF contests and the Illinois and California QSO Parties. California asks for its award lines here: type `YL`, `YOUTH`, `MOBILE`, `NEW CONTESTER`, `EXPEDITION`, `COUNTY-LINE EXPEDITION` or `ONE-DAY EXPEDITION`, one per line.
  - Neither Field Day, nor the Ohio or Texas QSO Party, has any of these in its template, so nothing new is written for them.
- Legacy contacts without a timestamp fall back to the `----------` placeholder rather than inventing a time.
- **Winter Field Day's** header is the one in the sponsor's example log: `CONTEST: WFD`, `LOCATION` (your section, `MX` or `DX`), `CATEGORY-POWER` (`QRP` or `LOW`, from Power category; HIGH writes none), `CLAIMED-SCORE` (the claimed total), `CLUB`, `OPERATORS`, `NAME`, `EMAIL` and `X-EXCHANGE` (your class and category, such as `3O`). Name, club, other operators and email come from Settings → Station and Settings → Contesting ▸ Your entry, each line left out when blank. A club's file is the host's **Club Cabrillo**, with an OPERATORS line naming whoever the positions logged under, then the host's other operators.
- **ARRL Field Day's GOTA station:** in the host's **Club Cabrillo**, each contact the GOTA laptop logged carries the GOTA station's call (that laptop's Callsign on the air) as the call sent, and every other contact the club's call. The rules require the GOTA station to use a call of its own (rule 4.1.1.1) and let the club claim its contacts (rule 4.1.1.5). The header stays the club's: the GOTA call goes on ARRL's online entry form, which has a box for it.

### ADIF

- Tags written per contact: `CALL`, `MODE`, `BAND`, `CONTEST_ID` (ARRL-FIELD-DAY or WFD), `CLASS`, `ARRL_SECT`, `<EOR>`.
- `PROGRAMID` is `Nexus`.
- `MODE` is the mode actually worked: CW maps to `CW`, Phone to `SSB`, and a digital contact carries its real mode (`FT8`, `FT4`, `RTTY`, …). Only legacy digital rows logged before the actual mode was recorded fall back to `FT8`.

### Score Summary

A one-page plain-text score summary: QSO counts by mode and by band, the sections worked, power multiplier, claimed bonuses and the score math (Winter Field Day lists the objectives completed with their multipliers, the OM and the claimed total). Hand it to the club scorekeeper or check your entry against it before submitting.

It also has a **QSOs by band and mode** table. For ARRL Field Day it is laid out as item 18 of ARRL's summary sheet: CW, Digital and Phone across, and 160 M, 80 M, 40 M, 20 M, 15 M, 10 M, 6 M, 2 M, 1.25 M, 70 CM, Other (any other band, 902 MHz and up among them), Satellite and GOTA down, with the totals that go on the sheet's lines 8, 9 and 10. A contact made through a satellite is counted under Satellite, which the rules list as a separate band. The GOTA row reads `—` because a laptop's log does not tell the GOTA station's contacts from its own: take them from the GOTA station's own summary. Every other contest's Summary has the same table in its own modes, one row per band its log holds: the Illinois QSO Party's CW and digital share a column because it counts them as one mode, and a contest that does not count modes apart (CQ WW, Sweepstakes, the VHF contests) shows the modes its log holds. Every contact in the log is counted, a duplicate logged at zero points included, and the Summary says how many of those there are.

### Dupe / Multiplier Sheet

A plain-text check sheet: every section multiplier with the call and band that first earned it, then an alphabetical callsign list showing how many times and where (band/mode) each station was worked, with dupes flagged `*`.

**ARRL Field Day:** submit the Cabrillo file through the ARRL's online submission system. **Winter Field Day:** submit on winterfieldday.org: fill in the form, which asks for your completed objectives, and upload the Cabrillo or ADIF file, named after the callsign used in the event (`K4SCO.log`, for example), by **23:59 UTC on 1 March 2027**. ADIF can be imported into N3FJP or other loggers for cross-checking.

---

## Limits / Not Yet

- **No warning on 12, 17, 30 or 60 m at Winter Field Day.** They are not WFD bands, but the log strip's band warning is off for both Field Days, so the checklist above is what says so.
- **Winter Field Day's objectives are yours to tick.** The hints for six bands, twelve bands and multiple modes count what your log shows; nothing is ticked for you, and the sponsor decides what it credits.
- **N3FJP errors are not surfaced in the UI** beyond the initial Test button; monitor N3FJP's own display to confirm pushes are landing.
- **A QSO party, Sweepstakes or CQ WW CW/SSB contact reaches N3FJP without its exchange.** N3FJP's API page does not say which boxes those programs keep the exchange in (it gives the state QSO parties as "Varies"), so Nexus sends the call, band and mode and fills nothing it would have to guess at.
- **N1MM is emit-only**: Nexus does not receive inbound `<contactinfo>` from other network stations.
- **Legacy digital rows export as FT8**: contacts journaled before the actual on-air mode was recorded have no mode on file, so ADIF and the interop push fall back to `FT8` for them. New digital contacts carry the mode actually worked.
- **TempoFast auto-sequencer requires operator initiation**: fully unattended automated operation is not implemented, consistent with ARRL FD rules requiring operator presence.
- **Club sync does not run a serial-number contest or CQ World-Wide.** It runs every other contest on the picker under that contest's own rules. With Sweepstakes, CQ WPX, the California QSO Party or CQ WW selected a station neither hosts nor joins, and the contest screen and Settings say why: one entry's serial numbers must run in a single sequence, and CQ WW's log must say which transmitter made each contact. Log those on each position by itself.
- **A removed contact stays wherever it already went.** N3FJP has no delete command, and Nexus does not send N1MM's contact-delete broadcast yet, so delete it in those programs yourself.
- **A contact removed before it was sent on, then restored after a later contact went out, is not sent on.** Add it in N3FJP by hand if your club uses it.
- **Going back to an older Nexus** brings removed contacts back as ordinary contacts, except one whose station you worked again on that band and mode, which the older version leaves out.
- **Desktop-only** (Tauri v2); no mobile companion.

---

## Other Contests, and CQ WW RTTY

The same workspace runs every contest on the **Settings → Contesting ▸ Contest** picker, not just the two Field Days: Sweepstakes, the ARRL VHF contests, CQ World-Wide DX and WPX, the CQ World-Wide RTTY DX Contest and six state QSO parties. Pick one, then turn on **Contest mode**, which is the switch for every contest: at the top of the **Contest** screen (the tent in the left bar), or in Settings → Contesting ▸ Contest, under the picker. If the contest cannot start yet, the Contest screen says what is missing and leaves the switch off. The log strip, the contest screen and the exports then follow that contest's rules. With Contest mode on, saving a different pick switches at once, exactly as turning the mode off and on does: the running contest ends (a CQ run with it), the new one starts in Search-and-Pounce without calling CQ, each contest's contacts stay apart in the journal, club sync starts again for the new contest, and the Contest screen says "Now running …" and names the contest that ended. Saving anything else keeps the running contest as it is. Class, section, the power tiers and the bonus checklist belong to Field Day and stay out of the way.

**CQ WW RTTY** (last full weekend of September, 0000Z Saturday for 48 hours):

- **Before the weekend**, set your **CQ zone** and your **State or province** under Settings → Contesting ▸ Your station data. Stations in the continental USA and Canada send `599`, their zone and their state or Canadian call area, using the sponsor's own codes (`NF`, `LB`, `NWT` and `PEI` among them; `NT` and `PE` typed there are read as `NWT` and `PEI`); everyone else sends `599` and their zone. If your callsign is in the US or Canada but the state is blank or not on the sponsor's list, you would send the DX exchange with no QTH: Nexus warns you in Settings, on the log strip and when the contest starts, and suggests a code where a section name points to one. It is a warning only, because operating from outside the US and Canada really is DX. The Contest section shows the exchange you are about to send. Set **Power category** and **Spotting assistance** under Contest too, because the Cabrillo header declares them.
- **The log strip** asks for RST (599 by default), the zone and the QTH, in a box captioned **State** (a USA or Canada station's state or province). The QTH can stay blank for a DX station. Type a call and the zone box shows that call's usual CQ zone from the country file as a faint hint; it never fills the box for you. A USA or Canada station logged without a QTH gets a warning, not a refusal. A QTH that is not on the sponsor's list (an ARRL section like `EMA`, a typo) still logs, but it counts as no multiplier. If the rig is on a band the contest does not use (it runs on 80, 40, 20, 15 and 10 m), the strip says so and still logs the contact.
- **Scoring** follows the sponsor: 3 points between continents, 2 between countries on the same continent, 1 within your own country. Zones, countries and W/VE QTHs each count once per band, and the USA and Canada count as countries. Each station can be worked once per band.
- **RTTY Auto** does not run in this contest. Send your exchange with the macros and log each contact in the strip.
- **The Cabrillo export** follows the sponsor's template: `CONTEST: CQ-WW-RTTY`, the dial you were on when you logged each contact, a two-digit zone, and `DX` in the QTH column where a station sent none. The headers carry CATEGORY-ASSISTED, -BAND, -MODE and -POWER, CLAIMED-SCORE, your operator name as NAME, the **Email for contest logs** setting as EMAIL and the **Club** setting as CLUB (each left out when blank). When you set them under Contest, the header also carries a multi-operator entry's class (`ONE`, `TWO` or `UNLIMITED`, with the `OPERATORS` line), the `CLASSIC` or `ROOKIE` overlay, your mailing address and your soapbox. LOCATION is your state or province in the sponsor's LOCATION spelling (`PEI` becomes `PE`, for example), or `DX`. A log with contacts on one band is declared single band, as the sponsor classifies it, unless you set **Band category** under Settings → Contesting ▸ Your entry: if you entered one band but also logged others, set it to that band and the file says so, and then writes no CLAIMED-SCORE, because the sponsor scores only that band. The Contest screen names a line the sponsor requires that the file would leave out, as it does for CQ WW and WPX below.

**CQ WW CW and SSB, and CQ WPX CW and SSB** (CQ WW SSB 24–25 October 2026, CQ WW CW 28–29 November 2026):

- **The Cabrillo header is the one on the sponsors' own Cabrillo pages** (cqww.com/cabrillo.htm, cqwpx.com/cabrillo.htm), each line from your own Settings and none of them guessed:
  - `CATEGORY-OPERATOR`: **Entry category**, under Settings → Contesting ▸ Your entry, like everything below unless it says otherwise.
  - `CATEGORY-ASSISTED`: **Spotting assistance**. `CATEGORY-POWER`: **Power category**. A contest reads both when Contest mode turns on, so set them before it starts.
  - `CATEGORY-BAND`: **Band category**: `ALL`, or the one band of a single-band entry. A single-band entry still logs every contact, other bands included, and CQ scores only the band named. A multi-operator entry is `ALL`.
  - `CATEGORY-MODE`: the contest's own, `CW` or `SSB`.
  - `CATEGORY-TRANSMITTER` and `OPERATORS`, for a `MULTI-OP` entry: **Multi-operator class**, and everyone set as **Operator at the key** when they logged a contact, then **Other operators**.
  - `CATEGORY-OVERLAY`, the `ADDRESS` lines and `SOAPBOX`: **Overlay**, **Mailing address** and **Soapbox**.
  - `CLUB`: **Club** (spell out the club's full name, as CQ asks). `EMAIL`: **Email for contest logs**. `NAME`: your operator name, under Settings → Station.
  - `CLAIMED-SCORE`: the score the Contest screen shows. That score counts every band, and CQ scores only the declared band of a single-band entry, so a single-band entry with a contact on any other band writes no claimed score; CQ works it out.
  - `LOCATION`: your **State or province** under Settings → Contesting ▸ Your station data, or `DX`.
- **A line you leave blank is left out, never filled in.** When CQ requires one you have not set (the assistance, the band and the power, and a multi-operator entry's class and operators), the Contest screen names it before you export, with a button to Settings → Contesting ▸ Your entry, and says so before Contest mode is on too. If it names Power category or Spotting assistance while the contest runs, set them, then turn Contest mode off and on again: the contest reads them as it starts, and your contacts stay in the log. A checklog is placed in no category, so nothing is named for one.
- `CLAIMED-SCORE`, `GRID-LOCATOR` and `CERTIFICATE` are not written. Add a claimed score by hand if you want to claim one; the other two are optional, and the certificate is `YES` when the line is left out.

**Illinois QSO Party** (1700Z Sunday of the third full weekend of October, for eight hours):

- **Before the party**, set your **State or province** under Settings → Contesting ▸ Your station data, and your **County** as well if you are in Illinois. Illinois stations send `599` and their county; everyone else sends `599` and their state, province or **country** — this party asks DX stations for their country rather than the word `DX`, so type the country into the state box. The Contest section shows the exchange you are about to send.
- **As a mobile or rover**, press **I moved** on the log strip when you cross into another county, type the new county's code and press Enter. From the next contact on you send and log the new county, a station you worked from the old county is a new contact again, and the new county becomes your **County** in Settings, so a restart keeps it. On a county line, send both counties yourself and log each contact once per county, switching between them with **I moved**.
- **Use the sponsor's county abbreviations.** The sponsor has said it will stop accepting non-standard ones, and names the pairs it sees mixed up: White (`WHIT`) and Whiteside (`WTSD`), Mason (`MASN`) and Macon (`MACN`). The log strip only ever writes the sponsor's own codes, so a county typed there by name is logged the right way. Check any you type by hand elsewhere.
- **The log strip** asks for RST and one box for the other station's county, captioned **County / State** when you are in Illinois (you also hear states, provinces and countries there) and **IL county** when you are not. That box takes the **county code or the county name**: type `Cook` and it offers `COOK`, `stclair` offers `SCLA`. Press space and the name becomes the code that goes in the log. Space also moves you on to the Call box, so type a two-word name without its space (`stclair`, `rockisland`), or type its first word and click the county in the list. A **half-typed name is not completed for you** and neither is one that could be several counties — `Ma` is Macon, Macoupin, Madison, Marion, Marshall, Mason and Massac — so pick from the list or finish typing. Nothing is ever guessed onto the air: the same rule is why a state name that several ARRL sections cover (New York, California) is never turned into one of them in Field Day's section box. From a station in the USA (Alaska and Hawaii included) or Canada, the box takes only a county, state or province on the sponsor's lists: anything else, such as `COKO`, `DUP` or `IL`, gets a line under the box saying so, and **Log** stays off until you fix it. When what you typed is a name or the start of one, the line offers the codes it may mean: `Washington` is the county `WASH` or the state `WA`, and `ST` may be `STAR`, `STEP` or `SCLA` (St. Clair). A DX station's country logs as it is typed.
- **Bands**: 160 through 2 metres, **without the WARC bands** (the sponsor's own list excludes 60, 30, 17 and 12 m). If the rig is elsewhere the strip says so and still logs the contact.
- **FT8 and FT4 earn no credit at all** — the sponsor's own rule, because of what an ILQP log entry has to contain. Other digital modes are encouraged; RTTY and PSK are ordinary digital contacts here. Nexus warns you, still logs the contact and writes it in the files, and counts it for nothing: no points, no county and no bonus, on your screen, on the club line or in the claimed score.
- **Power**: the high and low power classes split at **100 watts PEP** (it was 200 W before 2026). QRP is 5 W on CW and digital or 10 W on phone.
- **Spotting** is encouraged, and since 2026 every entrant may spot themselves.
- **A station counts once per band and mode, and CW and digital are ONE mode here**: work somebody on CW and the strip shows them as a dupe on RTTY on that band. Phone is separate. An Illinois mobile or rover in a new county is a new contact either way. A station **on a county line counts once per county**, two to four of them. Type its counties into the county box joined by `/`, the way the station sends them: `COOK/DUPG`, or up to four, like `COOK/DUPG/KANE/WILL`. A `+` joins them too: `COOK+DUPG` is the same line, and the box shows it as `COOK/DUPG` once you leave it. Press Enter and Nexus logs one contact per county, all at the same time, band and mode. Each county can be typed as its code or its name, and the list offers the counties of the part you are typing. While you type, the Dupe warning names the counties already worked on that band and mode: `Dupe: … on 40m CW for COOK and WILL` when every county of the line is one, `COOK is a dupe; WILL is new` when only some are. Enter logs every county, and a dupe is logged marked for 0 points, as a single contact is. A part that is not a county on the sponsor's list, such as a typo, a state or a half-typed name, stops the whole line until you fix it. Each county becomes an ordinary contact in the log, the same as one you logged on its own. The Satellites log strip still takes one county per contact. **Repeater contacts do not count.**
- **Scoring**: phone 1 point, CW and digital 2. Illinois stations multiply by Illinois counties plus US states, Canadian provinces and up to **five** DXCC entities (Canada, Hawaii and Alaska are not DX entities here); everyone else multiplies by the Illinois counties worked. A province counts once whichever spelling its station sends: `PE` and the sponsor's `PEI` are both Prince Edward Island, and `NL` counts as the sponsor's `NF`. Each contact is still logged and exported the way its station sent it, and Prince Edward Island typed by name in the county box is logged as `PEI`. The sponsoring club's two calls, **W9AWE** and **W9OAB**, are worth **100 bonus points each, once per log**, and Nexus adds them to your score and to the Cabrillo claimed score as soon as they are in the log — there is no box to tick.
- **The Cabrillo export** writes `CONTEST: ILLINOIS QSO PARTY`, which is what the sponsor's own sample log generates. Other loggers write `IL-QSO-PARTY` and the club plainly accepts those too, so change it if you prefer. An Illinois entry also gets `IL-COUNTY:` with your county's **name**, beside QSO lines carrying its four-letter code, exactly as the sample log does. The ADIF export uses ADIF's own contest name, `IL QSO Party`.
- **The header lines the sponsor's software reads.** The 2026 rules ask you to make sure the entry class, call sign, station location and club are right in the header, because the sponsor's processing software reads them from it. Under Settings → Contesting ▸ Your entry, set:
  - **Entry class**, one of the eight in the 2026 rules: Illinois fixed high or low power, Illinois portable, mobile or rover, outside Illinois high or low power, or **Unlimited**. A club running more than one transmitter at the same time enters Unlimited. It goes on the `ENTRY-CLASS:` line.
  - **Club**, for the `CLUB:` line.
  - **Email for contest logs**, for the `EMAIL:` line. The sponsor takes logs by email.
  - **Other operators**, for anyone who operated without being set as Operator at the key. The `OPERATORS:` line lists everyone who was set as Operator at the key when they logged a contact (Settings → Contesting ▸ Who's who at this event, or the operator box on the contest screen), then these.
  - **Power category** QRP, for a QRP entry. The file then says `QRP-COMPETITION: YES`.
  - **Mailing address** and **Soapbox**, if you want them in the file. The sponsor's sample log carries `ADDRESS`, `ADDRESS-CITY`, `ADDRESS-STATE-PROVINCE`, the postal code and `SOAPBOX` lines; a country you type rides one more `ADDRESS` line, because the sample has no line of its own for it. Left blank, none of them is written.

  All of these are read when you export, so you can set them after the party. While **Entry class** or **Email for contest logs** is blank, the Contest screen names it before you export, with a button to Settings → Contesting ▸ Your entry; an Entry class counts only when it is one of the eight, and the file still exports either way. The address lines in the sponsor's sample are written when **Mailing address** is set under Settings → Contesting ▸ Your entry.
- **Club sync runs this party.** One position hosts and the others join, as at Field Day, and the club log keeps the party's rules: every contact's county, CW and digital as one mode, a mobile's new county as a new contact, the Illinois multipliers with the five-country cap, the two bonus calls once for the whole club. A position typing a station another position already worked from that county is warned before it logs. The host's **Club Cabrillo** is the file to send: `CONTEST: ILLINOIS QSO PARTY`, `CATEGORY-OPERATOR: MULTI-OP`, the host's Entry class, Club and `IL-COUNTY`, an `OPERATORS` line naming everyone set as Operator at the key on any position (then the host's Other operators), and both counties on every QSO line. Every position must have the Illinois QSO Party picked: a position logging another contest is refused when it joins, and its club chip says so. Try it on two of your own PCs first (the checklist below); if anything there does not work, log on each position by itself and merge the files, with the second checklist.
- **Logs are due by midnight Central Time on 4 November 2026**, by email to n9jf@arrl.net, as a Cabrillo file. The sponsor asks for Cabrillo only: do not send an `.adi` or `.adif` file.

**Running the Illinois QSO Party as a club with club sync:**

Before the party, on every laptop:

- [ ] Settings → Contesting ▸ Contest: **Illinois QSO Party**, then **Contest mode** on (a contest picked while Contest mode is already on starts when you save it, and the Contest screen says "Now running Illinois QSO Party").
- [ ] Your station data: State **IL** and the club's **County** code. The **same callsign** on every laptop: the club file is written under the host's. A laptop set up out of state, or on another callsign, is refused when it joins, and both its club block and the host's say why. One set up on another county joins, and once it has logged a contact the host's club block names it, with the county it sends and the club's.
- [ ] Your entry: Entry category **MULTI-OP**, your **Power category**, **Entry class** (Unlimited if more than one position transmits at once), **Club**, and Email for contest logs.
- [ ] A **Position name** for each laptop, and **Operator at the key** for whoever is sitting there.
- [ ] One laptop: an **Event name** for the party and **Host a club event** on; its Contest screen then shows the address to join after **Positions join at**. The others: **Find club events**, or that address in **Join event at**, and check that each says it joined. On each Windows laptop, press **Allow Nexus through Windows Firewall** in Settings ▸ Contesting.
- [ ] **The same Nexus on every laptop.** The host turns away an older Nexus by name ("Update Nexus on this laptop"), and an older host turns away this one. Set each laptop up on its own rather than copying one laptop's settings folder to another: each Nexus makes its own club position and key the first time it starts, and the host takes each position only from the laptop that first joined as it.
- [ ] Every clock set from one source, to the second; once the positions have joined, the host's club board says each one is in step.
- [ ] For a TV in the room, **Spectator scoreboard** on the host, and on the laptop the TV is plugged into if that is a position: it shows the host's board, the counties map included.
- [ ] Rehearse before 1700Z on Saturday 17 October, a day before the party: the party's laptops then load none of the rehearsal's contacts, and their Contest screens say how many they kept out. The host leaves them out of the club's log too, whatever its Event name.
- [ ] **No test contacts from 1700Z on Saturday.** From then on, every contact logged with Contest mode on is in the party's log, and so in the club's file and its claimed score, even before the party starts. If one is logged, take it back at once with **Ctrl+D** twice on the laptop that logged it, while it is still that laptop's newest contact; an older one comes out of the Club Cabrillo file by hand before you send it.

During the party:

- [ ] One transmitted signal per position. Every club chip reads **Synced**; a chip reading Offline or Behind catches up by itself when the network is back.
- [ ] Whoever takes a seat sets **Operator at the key** first.

After the party:

- [ ] Every laptop's club chip reads **Synced** before anyone turns Contest mode off, so the club's log has every contact.
- [ ] On the host, **Club Cabrillo** before **Contest over?**: export it while Contest mode is still on, because its button is on the Contest screen only then. Read the header: `CALLSIGN` (the club's call), `ENTRY-CLASS`, `CLUB`, `OPERATORS`, `IL-COUNTY`, and your address and soapbox if you set them, and send that one file to n9jf@arrl.net by midnight Central Time on 4 November 2026. Export every position's own Cabrillo too, and keep them as a backup.

**Running the Illinois QSO Party as a club, with each position logging on its own** (if club sync is not used):

Before the party, on every laptop:

- [ ] Settings → Contesting ▸ Contest: **Illinois QSO Party**.
- [ ] Your station data: State **IL** and your **County** code.
- [ ] Your entry: Entry category **MULTI-OP**, your **Power category**, **Entry class** (Unlimited if more than one position transmits at once), **Club**, and Email for contest logs. Your operator name is on the Station tab.
- [ ] **Contest mode** on (Settings → Contesting ▸ Contest, or the top of the Contest screen). Leave **Host a club event** off and **Join event at** empty.
- [ ] Rehearse before 1700Z on Saturday 17 October, a day before the party: its contacts then stay out of the party's log, and the Contest screen says how many it kept out.
- [ ] Rehearse on each position: log a phone contact and a CW contact, log one of them again and see it refused as a dupe, then export a Cabrillo file and read its header.
- [ ] **No test contacts from 1700Z on Saturday.** From then on, every contact logged with Contest mode on is in the party's log and its Cabrillo file, even before the party starts. If one is logged, take it back at once with **Ctrl+D** twice, while it is still the newest contact; an older one comes out of the file by hand before you send it.

During the party:

- [ ] One transmitted signal per position.
- [ ] Agree out loud which band and mode each position works. Positions logging on their own get no warning when another position has already worked a station.
- [ ] Whoever takes a seat sets **Operator at the key** first.

After the party:

- [ ] Export every position's Cabrillo file.
- [ ] Make one file: keep the first file's header, add the `QSO:` lines of every other file, and end with `END-OF-LOG:`.
- [ ] Check the header: `ENTRY-CLASS`, `CLUB`, and one `OPERATORS` line naming everyone from all the files, and your address and soapbox if you set them. The claimed score is one position's; correct it or delete the line, because the sponsor rescores every log.
- [ ] Send that one file to n9jf@arrl.net by midnight Central Time on 4 November 2026.

**New York QSO Party** (the third Saturday of October, 1400Z for twelve hours: 17 October 2026, 10 AM to 10 PM Eastern):

- **Before the party**, set your **State or province** under Settings → Contesting ▸ Your station data, and your **County** as well if you are in New York, as the sponsor's three-letter code (`MON` for Monroe, `STL` for St. Lawrence; the county checklist on nyqp.org lists all 62). New York stations send `599` (59 on phone) and their county. Stations elsewhere in the US and Canada send their state or province, and everyone else sends `DX`: type `DX` in the state box. The Contest section shows the exchange you are about to send.
- **The log strip** asks for RST and one county box, captioned **County / State** in New York and **NY county** outside it, which takes the county code or the county name: type `Monroe` and it becomes `MON`. Three New York counties share a name with a state or a province, **Washington, Delaware and Ontario**, so those names are never filled in for you. Pick the county (`WAS`, `DEL`, `ONT`) or the state or province (`WA`, `DE`, `ON`) from the list.
- **Bands**: every US amateur band except 30, 17 and 12 m. **60 m counts here**, and so does everything from 6 m up (the sponsor's own sample log works 50, 144, 222, 432 and 902 MHz, 1.2 and 10 GHz). If the rig is elsewhere, the strip says so and still logs the contact.
- **FT8 and FT4 earn no credit.** The rules allow only digital modes that can carry the NYQP exchange, and the FT8 and FT4 contest messages have no room for a county. RTTY and PSK are fine. Nexus warns you, still logs the contact and writes it in the files, and counts it for nothing.
- **A station counts once per band in each of three modes**: phone, CW and digital, with every digital mode counting as one. A New York station that changes county is a new station, and so are you once you change county. A station **on a county line** sends both counties (`DUT/PUT` on CW): log it as **two contacts**, one per county, which the dupe rule allows. No more than two counties count for one contact. Nexus sends and logs one county at a time, so to operate from a line yourself, send both counties by hand and log each contact once per county, switching between them with **I moved** on the log strip.
- **Duplicates stay in the log.** The sponsor asks you not to remove them, because they are used for cross-checking. Nexus logs a repeat contact, marks it, and scores it zero.
- **Scoring**: phone 1 point, CW 2, digital 3. New York stations multiply by New York counties, US states and Canadian provinces, up to 125, and **New York itself counts as a state from your first New York county**. DX contacts count for points only. Everyone else multiplies by the New York counties worked, up to 62. There are no bonus points.
- **Spotting**: spot other stations freely, but **spotting yourself is allowed only for Mobile and Portable entries**. Nexus does not stop you, so keep it in mind.
- **The Cabrillo export** writes `CONTEST: NY-QSO-PARTY`. `LOCATION` is your **county code** for a New York entry (a mobile's is the county its first contact was made from), your state or province otherwise, or `DX`. RTTY contacts are marked `RY` and other digital modes `DG`, as the sponsor asks. The header carries the claimed score, your declared power and assistance, NAME and EMAIL, and, when you set them under Settings → Contesting ▸ Your entry, your mailing address (all five of the sample's `ADDRESS` lines), an overlay (`ROOKIE`, `YOUTH12`, `YOUTH17` or `YL`, the only values the rules accept) and, for a multi-operator entry, its class (`ONE` for Multi-One, `UNLIMITED` for Multi-Multi) with the `OPERATORS` line. `CLUB` and the station category are not written; the sponsor's upload form asks for the categories too.
- **Logs are due by 0200Z on 1 November 2026**, uploaded at nyqp.contesting.com.

---

*Previous: [Operating Guide](Operate-FT8-FT4.md) — Next: [Rig and Audio Setup](Rig-and-Audio-Setup.md)*
