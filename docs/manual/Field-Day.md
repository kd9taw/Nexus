# Field Day

Nexus has a dedicated Field Day workspace that covers ARRL Field Day (June) and Winter Field Day (January) from a single settings switch, with dupe-checked all-mode logging, a live bonus checklist, real-time N3FJP TCP push and N1MM UDP broadcast, and Cabrillo 3.0 / ADIF export ready for ARRL submission.

---

## Event Switch and Date Rules

In **Settings → Contesting ▸ Contest**, choose between:

| Setting value | Event | Window |
|---|---|---|
| _(empty, default)_ | ARRL Field Day, the 4th full weekend of June | **27 hours**: 1800 UTC Saturday → 2100 UTC Sunday |
| `wfd` | Winter Field Day, the 4th full weekend of January | **30 hours**: 1600 UTC Saturday → 21:59 UTC Sunday |

A full weekend has both days in the month, so a Saturday whose Sunday falls in February does not count. Winter Field Day 2027 is **23 and 24 January**, the sponsor's own dates. Its rules said "the last full weekend" until 2025, and the two readings differ in a January with five full weekends: 2027 and 2028 are both such years, and in each the last full weekend is a week later than the event. If a sponsor moves a date, a rules update carries it (Settings → Contesting ▸ Field Day Setup ▸ Check for rules updates), and it applies at the next launch.

### Countdown

The banner at the top of the **Contest** screen (the tent item in the left bar) shows a live countdown as the event approaches:

- **starts in N days** / **starts tomorrow** / **starts in Nh** / **starting soon** / **active**

The header reads **active** once the window opens and clears back to countdown after the event window closes (27 hours for ARRL FD, 30 hours for WFD).

---

## Pre-Event Checklist

Run through this before the weekend. Most problems are discovered Saturday at 1759 UTC, not Friday evening.

- [ ] **Set your class and section** in Settings → Contesting ▸ Field Day Setup (e.g. `3A`, `WI`). Both fields start **empty** — the greyed `1D` / `WI` you see are placeholder hints, not values. Field Day mode will not engage until both are filled in (the exchange goes on the air), and an export with blanks is malformed.
- [ ] **Set power multiplier** (ARRL Field Day): x5 (QRP/battery), x2 (≤100 W, the default), or x1 (>100 W). The engine clamps illegal values to the nearest legal tier.
- [ ] **Configure N3FJP** (see [N3FJP Setup](#n3fjp-setup) below) and press **Test** to confirm the handshake before the event.
- [ ] **Configure N1MM address** if your club runs N1MM dashboards (see [N1MM Broadcast](#n1mm-broadcast)).
- [ ] **Verify CAT / PTT** on all bands you plan to use. Use **Settings → Radio ▸ Rig & CAT** and the Test Tone / key-up checks on [Rig and Audio Setup](Rig-and-Audio-Setup.md).
- [ ] **Test the Phone and CW cockpits** end-to-end: make a test QSO on a non-event day to confirm the FD log strip accepts a manual entry and that the dupe toast fires on a repeat.
- [ ] **Claim bonuses** (ARRL Field Day) or **tick objectives** (Winter Field Day) on the contest screen as you achieve them (see [Bonus Checklist](#bonus-checklist) and [Winter Field Day Scoring](#winter-field-day-scoring)).

### Winter Field Day 2027

The 2027 event is **23 and 24 January**, 1600 UTC Saturday to 21:59 UTC Sunday. Before it, on every laptop:

- [ ] **Get the 2027 rules.** Settings → Contesting ▸ Field Day Setup ▸ **Check for rules updates**, then restart Nexus: a downloaded rules file takes effect at the next launch. Until then a 1.14 to 1.17 copy counts down to 30 January, a week late. **Nexus 1.13 or older cannot read the current rules file at all**: upgrade, or its Winter Field Day starts a week late.
- [ ] Settings → Contesting ▸ Contest: **Winter Field Day**, then **Field Day mode** on.
- [ ] **Class and category**: the number of transmitters you can run at once, then your category letter, `H` (home), `I` (indoor), `O` (outdoor) or `M` (mobile): `2O` is two transmitters outdoors. Then your ARRL/RAC section, `MX` in Mexico or `DX` anywhere else.
- [ ] **Power category** `LOW` (100 W PEP, the limit for everyone) or `QRP` (5 W on CW and digital, 10 W on phone, for the whole event). The Cabrillo file takes one of those two; HIGH writes no CATEGORY-POWER line.
- [ ] **Bands**: every amateur band **except 12, 17, 30 and 60 m**. Nexus does not warn on those four at Winter Field Day, so keep off them yourself.
- [ ] **Modes**: no WSJT mode (FT2, FST4, FT4, FT8, JT4, JT9, JT65, Q65, MSK144, WSPR, FST4W and Echo, the sponsor's list); the strip warns. JTTY is not on that list. RTTY, PSK and SSTV are digital contacts.
- [ ] **Only direct contacts count**: one band, one mode, simplex, station to station. No cross-band, repeater, relayed, meshed or internet-linked contact counts; a satellite contact is logged and kept, and scores nothing (see [Winter Field Day Scoring](#winter-field-day-scoring)).
- [ ] **Spot and solicit only over amateur RF.** While the event runs Nexus posts no spot over the internet (see [Winter Field Day Spotting](#winter-field-day-spotting)).
- [ ] For a club: allow Nexus through the Windows firewall on **Private** networks (club sync uses TCP 42073 and UDP 42074, the club TV scoreboard TCP 7373; Nexus adds no firewall rule of its own), and set **every clock from one source**, to the second. Nexus warns a position more than 30 seconds off the host's clock when it joins, and never sets a clock.
- [ ] Rehearse on an earlier day than the day before the event: entering the event loads only the contacts logged from 24 hours before it starts, keeps a rehearsal's out of its log by itself and deletes nothing (see [Contest Log Persistence](#contest-log-persistence)). On a host, use a different Event name from the rehearsal's, so the club journal starts fresh.

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

The WFD rules ban the entire WSJT-X mode suite (FT2, FST4, FT4, FT8, JT4, JT9, JT65, Q65, MSK144, WSPR, FST4W and Echo, the sponsor's own 2027 list) while explicitly keeping RTTY and SSTV legal as Digital. JTTY is not on that list. Nexus carries this list as advisory rules data. It does **not** block or disable any mode. Staying inside the rules is your call; what Nexus does guarantee is that a digital contact is exported and pushed under the mode actually used (an RTTY contact says RTTY, never FT8), so a legal contact can never be misreported as a banned one.

### Winter Field Day Spotting

The sponsor's 2027 rules: "You may spot yourself and others only via amateur RF." and "QSOs may be solicited only over amateur RF during the event." So while the event runs, a station with Winter Field Day switched on posts nothing over the internet: no PSK Reporter reports, no DX cluster spot from the Spot dialog, and no POTA self-spot, from the desktop or from Nexus Remote. The contest screen says so while it lasts, the Spot and Spot me buttons say why if you press them, and Settings says so beside PSK Reporter. Receiving cluster and skimmer spots carries on, your PSK Reporter setting is not changed, and everything posts again once the event ends. Asking for contacts anywhere but on the air is up to you to avoid.

---

## Scoreboard, Sections Board and Pop-Out

The Contest screen carries a live scoreboard: QSO and section counts, per-mode chips (DIG / CW / PH), and the score math for the active event (Winter Field Day shows its claimed total, QSO points × (OM + 1), never the ARRL power×+bonus formula).

- **Operator field** — Field Day rotates operators; type the call of whoever is at the key. It persists across restarts, and each QSO pushed to N3FJP is attributed to that operator (falling back to the station call when empty).
- **Sections board** — all 85 ARRL/RAC sections laid out division by division, each cell turning green with a ✓ as the section is worked, with a worked/total count. It doubles as your multiplier tracker.
- **Pop out** — the button in the scoreboard header tears the whole scoreboard (operator, tiles, sections board) off into its own window, sized for a second monitor or a club display facing the room. The docked view keeps working independently.
- **Club Board** — the **club band board** (position, band, mode, operator, QSOs, rate, clock) has its own button in the left rail, directly under Contest, and its own window. It appears whenever Field Day is on, whether or not club sync is running, and one click puts it on a second monitor: this is the board a multi-station club watches all event to see who is on what band before moving to another one. The same **Pop out board** button in the club header on the dashboard opens the same window. It is set in larger type than the docked copy because it is watched from the operating position rather than read at the keyboard, and it is a monitoring window: no operator field and no export buttons, both of which live on the dashboard.
- **A full board** (on the host) — every club line each position is sent carries the whole board, and one line carries about 59 positions. From the moment one more might not fit, the host's club block says how many positions the club has; past that, each position's board leaves out the positions heard from least recently, and says how many it shows. Contacts, dupe warnings and the score still sync.
- **Turned away** (on the host) — a position the host refuses when it joins (another contest, a Nexus too old for it, at a QSO party a laptop set up on the other side of the state line, or, in any contest but ARRL Field Day, a laptop on another callsign than the host's) is named on the host's club block with the sentence that position was sent, which its own club block shows too.
- **Clock** (on the host's board) — each position's clock against the host's, which the position measures every 5 s over the club link and reports to the host: *in step* under 2 s, how many seconds ahead or behind from there, in the warning colour past 30 s, and a dash for a position running an older Nexus. Each position's own club line says the same from 2 s ("This PC's clock is 3 s behind the host's") and warns past 30 s. Nexus only shows the difference and never changes a clock, so FT8/FT4 timing is untouched; put a wrong clock right in that PC's date and time settings.
- **With club sync off**, the window says so and names the route that turns it on (Settings ▸ Contesting ▸ Field Day Club Sync ▸ Host a club event) instead of showing an empty board. With sync on and nobody else logging yet, it says it is waiting.
- **Spectator scoreboard** — a web page for a TV or projector facing the room (Settings ▸ Contesting ▸ Field Day Club Sync ▸ **Spectator scoreboard**; the row shows the address to open on the TV). It reads across a room at 1080p and at 4K: the claimed score and how it is made, the rate, each position's band and mode, the contacts by band and mode, the latest contact, the time left, and a map of what the contest counts — the sections globe for both Field Days, the 102 counties for the Illinois QSO Party. It works for **any contest club sync runs**. The host shows its own club; a **position shows the host's board**, so the TV can sit at any table, as long as the host's Spectator scoreboard is on too, on the same port. If the host can't be reached the TV says so in plain words, keeps the last board, and comes back by itself; a station with no club says so, with what to do. Add `?theme=light` to the address for the light board, or `?theme=auto` to follow the TV.

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

### Digital (TempoFast auto-sequencer)

When the FD workspace is open and a digital contact is in progress, the TempoFast auto-sequencer handles the 4-step exchange autonomously once you initiate:

- **S&P** (Search-and-Pounce): double-click a CQ decode → sequencer sends your exchange → accepts their roger → logs the QSO.
- **Running**: answer an incoming exchange → roger with your exchange → accept their RR73 → log.

Opening **Contest** in the left bar with Field Day mode on always starts in **Search-and-Pounce**; with the mode off it opens the screen and nothing else. Switch to Running via the button pair in the Contest screen's header.

The WSJT-X UDP `Status` message sets `special_op = 3` (Field Day) while FD mode is active. Once the **WSJT-X UDP API** switch in Settings → Logging & Connectors ▸ Integrations & Feeds is on (it is off by default), JTAlert and GridTracker will automatically activate their FD-specific behavior without any other configuration on your end. FD contacts are also emitted as `QsoLogged` UDP datagrams to the same sink.

### CW

Navigate to the CW cockpit. The log strip detects that FD mode is active and shows **Class** and **Section** fields alongside the standard call/RST fields. Fill in the exchange and press Log — the entry routes to `fdLogManual()` with mode code `CW` and is dupe-checked against the FD log.

The CW cockpit pre-fills Class from the most recent FD entry so you do not retype it for every contact.

### Phone

Navigate to the Phone cockpit. The log strip similarly adds Class and Section fields and routes to `fdLogManual()` with mode code `PH` (1 point). RST defaults to 59.

All three mode classes write into the **same unified FD log**, so the live score and Cabrillo export reflect the full multi-mode total in real time.

### Contest Log Persistence

The contest log survives restarts: every logged contact is journaled to `fieldday_backup.adi` (beside `settings.json`), and the journal is restored automatically whenever you re-enter Field Day mode — a mid-event quit, crash, or Run/Search-and-Pounce switch loses nothing. Entries from a previous event (older than 4 days) are not restored, so the journal self-expires between events.

Only the session's own contacts are restored: the contest you are entering, logged from 24 hours before this running of it starts. A rehearsal on an earlier day, or another contest worked in the last four days, stays in the journal untouched and out of this log, and the Contest screen says how many contacts it kept out and why. A gear test within 24 hours of the start is part of the event's log; to start clean after one, delete `fieldday_backup.adi` (with Nexus closed, or at least outside Field Day mode — the next contact logged in FD mode re-writes the whole journal from memory).

### Take Back the Last Contact

Logged a contact you should not have, or stopped your TU after the contact was already in the log? Press **Ctrl+D** from anywhere in a cockpit that shows the contest log strip, or click **Remove last** beside Clear. The first press names your newest contest contact on the strip's message line, for example `Remove K9AAA · 20m CW · 17:32 · 599 COOK? Press Ctrl+D again.` A second press within 5 seconds removes it. Any other key, the 5 seconds running out, or a new contact cancels it, and if the newest contact changed between the two presses nothing is removed and the strip says so. A held-down Ctrl+D counts as one press. The caret stays where it was, and Ctrl+D never keys or stops the transmitter: Esc still stops TX. A contact on a county line goes with all its counties. On the contest screen the newest row of the log has its own **Remove**, which asks the same way: click it twice.

Nothing is deleted. The contact leaves the QSO count, the score, the multipliers, the rate, the dupe check, the Cabrillo, the contest ADIF, the summary and a later merge into your logbook, and the contest screen keeps it under **Removed**. **Restore** there puts it back exactly as it was, with the same time, serial number and club-sync number, unless you have worked that station again on that band and mode since; then it stays removed and the screen says why. A removed contact's serial number and club-sync number are never handed to another contact, not even after a restart, because the journal keeps the removed contact, marked as removed.

The contact may already have gone elsewhere, and Nexus cannot take it back from there. The strip says where: N3FJP, the N1MM broadcast and WSJT-X listeners, once a slot boundary had sent it on to them, and your logbook if you had merged the contest, with the services that copy was uploaded to. Delete it in those places yourself if you need to. Removing it from the contest log never touches your logbook. The other way round does work: deleting a contact in the Logbook that you merged from the contest still running also removes it from the contest log (it appears under Removed, and Restore brings it back), the way an edit of its call or band in the Logbook already corrects the contest log.

**Removal is refused while club sync is on**, at a position or at the host, because the club log cannot take a contact back yet: the strip says so, and the club's Cabrillo file is the place to fix it before you send it in. Removal is not offered on the hosted Remote page or in the Remote client.

Remember that most sponsors check logs against each other: if the other station logged you, a contact you remove becomes a not-in-log for them. When in doubt, keep it.

---

## Enter Sends Message (ESM)

With **Enter Sends Message** on, each press of Enter in the contest log strip sends the contact's next message from your F-keys, and the press for the contact's last step logs it, the way ESM works in N1MM Logger+. It works in the CW, RTTY and Phone cockpits while a contest runs. It is **off** until you turn it on, separately for each cockpit, in [Settings → Contesting ▸ Enter Sends Message](../guide/settings-reference.md#enter-sends-message) or with the ESM switch in the cockpit's TX dock.

### How a contact runs

ESM starts in search and pounce (S&P). Pressing the key that holds your CQ switches to running, and clicking a spot switches back to S&P.

| You are | The strip holds | Enter sends |
|---|---|---|
| Running | no call | your CQ |
| Running | a call, and your exchange has not gone to it | his call and your exchange; if his exchange is not complete yet, the cursor moves to his first exchange box |
| Running | your exchange has gone, his is not complete | AGN |
| Running | your exchange has gone, his is complete | TU, and the contact logs |
| S&P | no call, or a call whose exchange is not complete | your call |
| S&P | a complete exchange | your exchange, and the contact logs |

With **Call once (S&P)** on, your call goes once per station: the next Enter sends AGN instead. If you correct a busted call after your exchange went out, the corrected call gets your exchange again. In Phone, when you are running, Enter plays nothing for his call and your exchange: you say them yourself, because a recording cannot say a callsign, and Enter moves the cursor to his exchange.

His exchange counts as complete only with what you gave for this contact: what you typed or picked, the 5NN or 59 nobody changed, or a box filled from call history. A box filled from call history (marked **history**) is not accepted until you type it: at the step that would log the contact, Enter sends nothing and the strip names the box, and Alt+Enter logs it as it is. In Field Day, the class and section the strip keeps from your last contact count only once you type them for this one.

**In the TX dock**, beside the ESM switch, the plate shows **Run** or **S&P** (click it to switch) and what the next Enter does: the key or keys it sends, which glow on the dock, or why it sends nothing, or why ESM steps aside. Its tooltip reminds you that Alt+Enter logs without sending anything.

### The rules

1. ESM is off until you turn it on, separately in the CW, RTTY and Phone cockpits. Nothing turns it on for you, and it remembers your choice.
2. It works only in the contest log strip of those three cockpits while a contest runs. It does nothing in FT8, FT4 or any FT mode, nor in Digital, Tempo, PSK, SSTV or APRS, nor on the hosted Remote page.
3. Enter sends the message of the next step: CQ, his call and your exchange, TU, your call, your S&P exchange, or AGN. The message comes from your own keys where you mapped them to that step, and otherwise from the built-in set's layout, and it goes out by the same path, with the same checks, as that F-key would send it.
   - If your set has no message for the step, Enter sends nothing and logs nothing, and the strip names the step.
   - If your set has no step mapped at all, ESM steps aside: Enter logs as it does with ESM off.
4. Enter sends only when TX is already on, the dial is inside your license privileges, and no clock repair is running. In Phone it also needs you not to be holding PTT, the keyer not to be recording, and the radio not to have the mic. Otherwise Enter sends nothing and logs nothing, and the strip says why.
   - While the RTTY auto sequence runs or Continuous TX is latched, or while the Phone voice keyer is hidden, ESM steps aside: Enter logs as it does with ESM off, and says why.
5. Enter never turns TX on. After Stop TX, Esc or the watchdog, Enter is refused until you turn TX back on yourself: in CW by sending with an F-key, in RTTY with the TX button, in Phone with PTT.
6. One press, one message. Holding Enter sends once. Alt+Enter logs without sending anything.
7. The call it sends is the call in the strip.
8. The contact logs the moment you press Enter for its last step, as N1MM does: TU when you are running, your exchange in S&P. It logs only a complete exchange that is not a dupe of your own log, and only once that last message has started. An Enter that is refused logs nothing.
   - If you then stop that message, with Esc, Stop TX or the watchdog, the contact stays logged and the strip has already cleared for the next one. The strip says so. To take the contact back, press Ctrl+D twice: it leaves your contest log, score and exports, and you can restore it from the contest screen. While club sync is on it cannot be removed, because the club log already has it: edit the club's Cabrillo file before you send it in.
   - If you stop an earlier message of the contact (his call and your exchange, or your call), it counts as not sent, and the next Enter sends it again rather than logging.
9. On a dupe of your own log, Enter sends nothing and logs nothing. A club dupe (another position worked them) stays a warning, and Enter goes on as it would for a new call.
10. A Super Check Partial match or a call-history hint is never sent or logged until you accept it.
11. Esc and Stop TX stop it at any step, from anywhere in the cockpit.
12. Nothing is sent by a timer. Every message is one press of yours.

### Which key sends each step

The built-in sets are laid out the way N1MM lays them out: **F1** CQ · **F2** his call and your exchange · **F3** TU · **F4** your call · **F6** your S&P exchange · **F7** AGN. **F5** (his call alone) and **F8** (QSO B4) stay keys you press yourself; ESM never sends F8, because on a dupe it sends nothing. RTTY's Contest set uses this layout, and so do CW's contest sets and its Field Day set; the [CW manual](CW.md#eight-f-key-macros) lists their texts. RTTY's Everyday set and CW's everyday set have no steps.

The voice keyer's slots follow the same idea: **F1** CQ · **F2** your exchange · **F3** TU · **F4** your call · **F5** AGN. No slot holds his call and your exchange, because you say those yourself when you run, and **F6** stays yours. A slot with no recording plays nothing, and the strip names the slot to record.

**Your own keys.** Settings → Contesting ▸ Enter Sends Message lists the steps for each cockpit: the active CW profile, each RTTY set, and the voice keyer. For each step, pick the key that sends it, or two keys that go out as one message (F5 then F2, as an N1MM-style set sends his call and then the exchange). Phone takes one slot per step. A step you leave on *Built in* uses the built-in key, and each row shows what Enter would send there, or why it would send nothing. The mapping is saved with its set: a CW profile carries its own, and so does each RTTY set. A CW profile still on the built-in sets shows their steps and has nothing to map until you customize it in Settings → CW.

---

## Band Follows QSY

When you change frequency — whether via a software dial command or by turning the rig's VFO knob — the active FD log's band field updates immediately. You do not need to manually change a "current band" setting mid-event. Without this, a QSY between bands would stamp subsequent contacts under the wrong band in Cabrillo, corrupting dupe keys and the band-column breakdown.

---

## N3FJP Setup

N3FJP Field Day Contest Log is widely used by clubs as the master log. Nexus pushes each new FD QSO to N3FJP immediately after logging over TCP, using the `ADDDIRECT` command followed by `CHECKLOG` to refresh the N3FJP screen.

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
- `CATEGORY-OPERATOR` is the **Entry category** you pick in Settings → Contesting ▸ Contest (`SINGLE-OP` by default).
- Legacy contacts without a timestamp fall back to the `----------` placeholder rather than inventing a time.
- **Winter Field Day's** header is the one in the sponsor's example log: `CONTEST: WFD`, `LOCATION` (your section, `MX` or `DX`), `CATEGORY-POWER` (`QRP` or `LOW`, from Power category; HIGH writes none), `CLAIMED-SCORE` (the claimed total), `CLUB`, `OPERATORS`, `NAME`, `EMAIL` and `X-EXCHANGE` (your class and category, such as `3O`). Name, club, other operators and email come from Settings → Station and Settings → Contesting ▸ Contest, each line left out when blank. A club's file is the host's **Club Cabrillo**, with an OPERATORS line naming whoever the positions logged under, then the host's other operators.
- **ARRL Field Day's GOTA station:** in the host's **Club Cabrillo**, each contact the GOTA laptop logged carries the GOTA station's call (that laptop's Callsign on the air) as the call sent, and every other contact the club's call. The rules require the GOTA station to use a call of its own (rule 4.1.1.1) and let the club claim its contacts (rule 4.1.1.5). The header stays the club's: the GOTA call goes on ARRL's online entry form, which has a box for it.

### ADIF

- Tags written per contact: `CALL`, `MODE`, `BAND`, `CONTEST_ID` (ARRL-FIELD-DAY or WFD), `CLASS`, `ARRL_SECT`, `<EOR>`.
- `PROGRAMID` is `Nexus`.
- `MODE` is the mode actually worked: CW maps to `CW`, Phone to `SSB`, and a digital contact carries its real mode (`FT8`, `FT4`, `RTTY`, …). Only legacy digital rows logged before the actual mode was recorded fall back to `FT8`.

### Score Summary

A one-page plain-text score summary: QSO counts by mode and by band, the sections worked, power multiplier, claimed bonuses and the score math (Winter Field Day lists the objectives completed with their multipliers, the OM and the claimed total). Hand it to the club scorekeeper or check your entry against it before submitting.

### Dupe / Multiplier Sheet

A plain-text check sheet: every section multiplier with the call and band that first earned it, then an alphabetical callsign list showing how many times and where (band/mode) each station was worked, with dupes flagged `*`.

**ARRL Field Day:** submit the Cabrillo file through the ARRL's online submission system. **Winter Field Day:** submit on winterfieldday.org: fill in the form, which asks for your completed objectives, and upload the Cabrillo or ADIF file, named after the callsign used in the event (`K4SCO.log`, for example), by **23:59 UTC on 1 March 2027**. ADIF can be imported into N3FJP or other loggers for cross-checking.

---

## Limits / Not Yet

- **No warning on 12, 17, 30 or 60 m at Winter Field Day.** They are not WFD bands, but the log strip's band warning is off for both Field Days, so the checklist above is what says so.
- **Winter Field Day's objectives are yours to tick.** The hints for six bands, twelve bands and multiple modes count what your log shows; nothing is ticked for you, and the sponsor decides what it credits.
- **N3FJP errors are not surfaced in the UI** beyond the initial Test button; monitor N3FJP's own display to confirm pushes are landing.
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

The same workspace runs every contest on the **Settings → Contesting ▸ Contest** picker, not just the two Field Days: Sweepstakes, the ARRL VHF contests, CQ World-Wide DX and WPX, the CQ World-Wide RTTY DX Contest and six state QSO parties. Pick one, then turn on **Field Day mode**, which is the switch for every contest: at the top of the **Contest** screen (the tent in the left bar), or in Field Day Setup. If the contest cannot start yet, the Contest screen says what is missing and leaves the switch off. The log strip, the contest screen and the exports then follow that contest's rules. Class, section, the power tiers and the bonus checklist belong to Field Day and stay out of the way.

**CQ WW RTTY** (last full weekend of September, 0000Z Saturday for 48 hours):

- **Before the weekend**, set your **CQ zone** and your **State or province** under Settings → Contesting ▸ Your station data. Stations in the continental USA and Canada send `599`, their zone and their state or Canadian call area, using the sponsor's own codes (`NF`, `LB`, `NWT` and `PEI` among them; `NT` and `PE` typed there are read as `NWT` and `PEI`); everyone else sends `599` and their zone. If your callsign is in the US or Canada but the state is blank or not on the sponsor's list, you would send the DX exchange with no QTH: Nexus warns you in Settings, on the log strip and when the contest starts, and suggests a code where a section name points to one. It is a warning only, because operating from outside the US and Canada really is DX. The Contest section shows the exchange you are about to send. Set **Power category** and **Spotting assistance** under Contest too, because the Cabrillo header declares them.
- **The log strip** asks for RST (599 by default), the zone and the QTH. The QTH can stay blank for a DX station. Type a call and the zone box shows that call's usual CQ zone from the country file as a faint hint; it never fills the box for you. A USA or Canada station logged without a QTH gets a warning, not a refusal. A QTH that is not on the sponsor's list (an ARRL section like `EMA`, a typo) still logs, but it counts as no multiplier. If the rig is on a band the contest does not use (it runs on 80, 40, 20, 15 and 10 m), the strip says so and still logs the contact.
- **Scoring** follows the sponsor: 3 points between continents, 2 between countries on the same continent, 1 within your own country. Zones, countries and W/VE QTHs each count once per band, and the USA and Canada count as countries. Each station can be worked once per band.
- **RTTY Auto** does not run in this contest. Send your exchange with the macros and log each contact in the strip.
- **The Cabrillo export** follows the sponsor's template: `CONTEST: CQ-WW-RTTY`, the dial you were on when you logged each contact, a two-digit zone, and `DX` in the QTH column where a station sent none. The headers carry CATEGORY-ASSISTED, -BAND, -MODE and -POWER, CLAIMED-SCORE, your operator name as NAME, and the **Email for contest logs** setting as EMAIL (left out when blank). LOCATION is your state or province in the sponsor's LOCATION spelling (`PEI` becomes `PE`, for example), or `DX`. A log with contacts on one band is declared single band, as the sponsor classifies it. If you entered one band but also logged others, change CATEGORY-BAND by hand before you upload.

**Illinois QSO Party** (1700Z Sunday of the third full weekend of October, for eight hours):

- **Before the party**, set your **State or province** under Settings → Contesting ▸ Your station data, and your **County** as well if you are in Illinois. Illinois stations send `599` and their county; everyone else sends `599` and their state, province or **country** — this party asks DX stations for their country rather than the word `DX`, so type the country into the state box. The Contest section shows the exchange you are about to send.
- **Use the sponsor's county abbreviations.** The sponsor has said it will stop accepting non-standard ones, and names the pairs it sees mixed up: White (`WHIT`) and Whiteside (`WTSD`), Mason (`MASN`) and Macon (`MACN`). The log strip only ever writes the sponsor's own codes, so a county typed there by name is logged the right way. Check any you type by hand elsewhere.
- **The log strip** asks for RST and one QTH box, and that box takes the **county code or the county name**: type `Cook` and it offers `COOK`, `st clair` offers `SCLA`. Press space and the name becomes the code that goes in the log. A **half-typed name is not completed for you** and neither is one that could be several counties — `Ma` is Macon, Macoupin, Madison, Marion, Marshall, Mason and Massac — so pick from the list or finish typing. Nothing is ever guessed onto the air: the same rule is why a state name that several ARRL sections cover (New York, California) is never turned into one of them in Field Day's section box. A state, a province or a country typed there logs as it is.
- **Bands**: 160 through 2 metres, **without the WARC bands** (the sponsor's own list excludes 60, 30, 17 and 12 m). If the rig is elsewhere the strip says so and still logs the contact.
- **FT8 and FT4 earn no credit at all** — the sponsor's own rule, because of what an ILQP log entry has to contain. Other digital modes are encouraged; RTTY and PSK are ordinary digital contacts here. Nexus warns you and still logs.
- **Power**: the high and low power classes split at **100 watts PEP** (it was 200 W before 2026). QRP is 5 W on CW and digital or 10 W on phone.
- **Spotting** is encouraged, and since 2026 every entrant may spot themselves.
- **A station counts once per band and mode, and CW and digital are ONE mode here**: work somebody on CW and the strip shows them as a dupe on RTTY on that band. Phone is separate. An Illinois mobile or rover in a new county is a new contact either way. A station **on a county line counts once per county**, two to four of them. Type its counties into the QTH box joined by `/`, the way the station sends them: `COOK/DUPG`, or up to four, like `COOK/DUPG/KANE/WILL`. Press Enter and Nexus logs one contact per county, all at the same time, band and mode. Each county can be typed as its code or its name, and the list offers the counties of the part you are typing. A county already worked on that band and mode is not logged again; Nexus logs the others and tells you which county was the dupe. A part that is not a county on the sponsor's list, such as a typo, a state or a half-typed name, stops the whole line until you fix it. Each county becomes an ordinary contact in the log, the same as one you logged on its own. The Satellites log strip still takes one county per contact. **Repeater contacts do not count.**
- **Scoring**: phone 1 point, CW and digital 2. Illinois stations multiply by Illinois counties plus US states, Canadian provinces and up to **five** DXCC entities (Canada, Hawaii and Alaska are not DX entities here); everyone else multiplies by the Illinois counties worked. The sponsoring club's two calls, **W9AWE** and **W9OAB**, are worth **100 bonus points each, once per log**, and Nexus adds them to your score and to the Cabrillo claimed score as soon as they are in the log — there is no box to tick.
- **The Cabrillo export** writes `CONTEST: ILLINOIS QSO PARTY`, which is what the sponsor's own sample log generates. Other loggers write `IL-QSO-PARTY` and the club plainly accepts those too, so change it if you prefer. An Illinois entry also gets `IL-COUNTY:` with your county's **name**, beside QSO lines carrying its four-letter code, exactly as the sample log does. The ADIF export uses ADIF's own contest name, `IL QSO Party`.
- **The header lines the sponsor's software reads.** The 2026 rules ask you to make sure the entry class, call sign, station location and club are right in the header, because the sponsor's processing software reads them from it. Under Settings → Contesting ▸ Contest, set:
  - **Entry class**, one of the eight in the 2026 rules: Illinois fixed high or low power, Illinois portable, mobile or rover, outside Illinois high or low power, or **Unlimited**. A club running more than one transmitter at the same time enters Unlimited. It goes on the `ENTRY-CLASS:` line.
  - **Club**, for the `CLUB:` line.
  - **Other operators**, for anyone who operated without being set as Operator at the key. The `OPERATORS:` line lists everyone who was set as Operator at the key when they logged a contact (Settings → Contesting ▸ Who's who at this event, or the operator box on the contest screen), then these.
  - **Power category** QRP, for a QRP entry. The file then says `QRP-COMPETITION: YES`.

  All of these are read when you export, so you can set them after the party. The address lines in the sponsor's sample are not written; add them by hand if you want them.
- **Club sync runs this party.** One position hosts and the others join, as at Field Day, and the club log keeps the party's rules: every contact's county, CW and digital as one mode, a mobile's new county as a new contact, the Illinois multipliers with the five-country cap, the two bonus calls once for the whole club. A position typing a station another position already worked from that county is warned before it logs. The host's **Club Cabrillo** is the file to send: `CONTEST: ILLINOIS QSO PARTY`, `CATEGORY-OPERATOR: MULTI-OP`, the host's Entry class, Club and `IL-COUNTY`, an `OPERATORS` line naming everyone set as Operator at the key on any position (then the host's Other operators), and both counties on every QSO line. Every position must have the Illinois QSO Party picked: a position logging another contest is refused when it joins, and its club chip says so. Try it on two of your own PCs first (the checklist below); if anything there does not work, log on each position by itself and merge the files, with the second checklist.
- **Logs are due by midnight Central Time on 4 November 2026**, by email to n9jf@arrl.net, as a Cabrillo file. The sponsor asks for Cabrillo only: do not send an `.adi` or `.adif` file.

**Running the Illinois QSO Party as a club with club sync:**

Before the party, on every laptop:

- [ ] Settings → Contesting ▸ Contest: **Illinois QSO Party**, then **Field Day mode** on (a contest picked while Field Day mode is already on takes effect only after it is turned off and on again; the Contest screen says so, beside the switch).
- [ ] Your station data: State **IL** and the club's **County** code. The **same callsign** on every laptop: the club file is written under the host's. A laptop set up out of state, or on another callsign, is refused when it joins, and both its club block and the host's say why.
- [ ] Contest: Entry category **MULTI-OP**, your **Power category**, **Entry class** (Unlimited if more than one position transmits at once), **Club**, and Email for contest logs.
- [ ] A **Position name** for each laptop, and **Operator at the key** for whoever is sitting there.
- [ ] One laptop: an **Event name** for the party and **Host a club event** on. The others: **Find club events**, or the host's address in **Join event at**. Allow Nexus through the Windows firewall on Private networks.
- [ ] Every clock set from one source, to the second; once the positions have joined, the host's club board says each one is in step.
- [ ] For a TV in the room, **Spectator scoreboard** on the host, and on the laptop the TV is plugged into if that is a position: it shows the host's board, the counties map included.
- [ ] Rehearse before 1700Z on Saturday 17 October, a day before the party: the party's laptops then load none of the rehearsal's contacts, and their Contest screens say how many they kept out. On the host, use a different Event name from the rehearsal's, or its club journal brings the rehearsal's contacts back into the club's log.

During the party:

- [ ] One transmitted signal per position. Every club chip reads **Synced**; a chip reading Offline or Behind catches up by itself when the network is back.
- [ ] Whoever takes a seat sets **Operator at the key** first.

After the party, on the host:

- [ ] **Club Cabrillo**, then read the header: `ENTRY-CLASS`, `CLUB`, `OPERATORS`, `IL-COUNTY`. Add your address lines if you want them, and send that one file to n9jf@arrl.net by midnight Central Time on 4 November 2026. Export every position's own Cabrillo too, and keep them as a backup.

**Running the Illinois QSO Party as a club, with each position logging on its own** (if club sync is not used):

Before the party, on every laptop:

- [ ] Settings → Contesting ▸ Contest: **Illinois QSO Party**.
- [ ] Your station data: State **IL** and your **County** code.
- [ ] Contest: Entry category **MULTI-OP**, your **Power category**, **Entry class** (Unlimited if more than one position transmits at once), **Club**, and Email for contest logs. Your operator name is on the Station tab.
- [ ] Field Day Setup: **Field Day mode** on. Leave **Host a club event** off and **Join event at** empty.
- [ ] Rehearse before 1700Z on Saturday 17 October, a day before the party: its contacts then stay out of the party's log, and the Contest screen says how many it kept out.
- [ ] Rehearse on each position: log a phone contact and a CW contact, log one of them again and see it refused as a dupe, then export a Cabrillo file and read its header.

During the party:

- [ ] One transmitted signal per position.
- [ ] Agree out loud which band and mode each position works. Positions logging on their own get no warning when another position has already worked a station.
- [ ] Whoever takes a seat sets **Operator at the key** first.

After the party:

- [ ] Export every position's Cabrillo file.
- [ ] Make one file: keep the first file's header, add the `QSO:` lines of every other file, and end with `END-OF-LOG:`.
- [ ] Check the header: `ENTRY-CLASS`, `CLUB`, and one `OPERATORS` line naming everyone from all the files. Add your address lines if you want them. The claimed score is one position's; correct it or delete the line, because the sponsor rescores every log.
- [ ] Send that one file to n9jf@arrl.net by midnight Central Time on 4 November 2026.

**New York QSO Party** (the third Saturday of October, 1400Z for twelve hours: 17 October 2026, 10 AM to 10 PM Eastern):

- **Before the party**, set your **State or province** under Settings → Contesting ▸ Your station data, and your **County** as well if you are in New York, as the sponsor's three-letter code (`MON` for Monroe, `STL` for St. Lawrence; the county checklist on nyqp.org lists all 62). New York stations send `599` (59 on phone) and their county. Stations elsewhere in the US and Canada send their state or province, and everyone else sends `DX`: type `DX` in the state box. The Contest section shows the exchange you are about to send.
- **The log strip** asks for RST and one QTH box, which takes the county code or the county name: type `Monroe` and it becomes `MON`. Three New York counties share a name with a state or a province, **Washington, Delaware and Ontario**, so those names are never filled in for you. Pick the county (`WAS`, `DEL`, `ONT`) or the state or province (`WA`, `DE`, `ON`) from the list.
- **Bands**: every US amateur band except 30, 17 and 12 m. **60 m counts here**, and so does everything from 6 m up (the sponsor's own sample log works 50, 144, 222, 432 and 902 MHz, 1.2 and 10 GHz). If the rig is elsewhere, the strip says so and still logs the contact.
- **FT8 and FT4 earn no credit.** The rules allow only digital modes that can carry the NYQP exchange, and the FT8 and FT4 contest messages have no room for a county. RTTY and PSK are fine. Nexus warns you and still logs.
- **A station counts once per band in each of three modes**: phone, CW and digital, with every digital mode counting as one. A New York station that changes county is a new station, and so are you once you change county. A station **on a county line** sends both counties (`DUT/PUT` on CW): log it as **two contacts**, one per county, which the dupe rule allows. No more than two counties count for one contact. Nexus sends and logs one county at a time, so to operate from a line yourself, send both counties by hand and log each contact once per county, switching between them with **I moved** on the log strip.
- **Duplicates stay in the log.** The sponsor asks you not to remove them, because they are used for cross-checking. Nexus logs a repeat contact, marks it, and scores it zero.
- **Scoring**: phone 1 point, CW 2, digital 3. New York stations multiply by New York counties, US states and Canadian provinces, up to 125, and **New York itself counts as a state from your first New York county**. DX contacts count for points only. Everyone else multiplies by the New York counties worked, up to 62. There are no bonus points.
- **Spotting**: spot other stations freely, but **spotting yourself is allowed only for Mobile and Portable entries**. Nexus does not stop you, so keep it in mind.
- **The Cabrillo export** writes `CONTEST: NY-QSO-PARTY`. `LOCATION` is your **county code** for a New York entry (a mobile's is the county its first contact was made from), your state or province otherwise, or `DX`. RTTY contacts are marked `RY` and other digital modes `DG`, as the sponsor asks. The header carries the claimed score, your declared power and assistance, NAME and EMAIL. `CLUB`, `OPERATORS`, the address and the station, transmitter and overlay categories are not written, because Nexus has nothing to fill them from; the sponsor's upload form asks for the categories.
- **Logs are due by 0200Z on 1 November 2026**, uploaded at nyqp.contesting.com.

---

*Previous: [Operating Guide](Operate-FT8-FT4.md) — Next: [Rig and Audio Setup](Rig-and-Audio-Setup.md)*
