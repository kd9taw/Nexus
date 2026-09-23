# Field Day

Nexus has a dedicated Field Day workspace that covers ARRL Field Day (June) and Winter Field Day (January) from a single settings switch, with dupe-checked all-mode logging, a live bonus checklist, real-time N3FJP TCP push and N1MM UDP broadcast, and Cabrillo 3.0 / ADIF export ready for ARRL submission.

---

## Event Switch and Date Rules

In **Settings → Contesting ▸ Contest**, choose between:

| Setting value | Event | Window |
|---|---|---|
| _(empty, default)_ | ARRL Field Day — 4th full weekend of June | **27 hours**: 1800 UTC Saturday → 2100 UTC Sunday |
| `wfd` | Winter Field Day — last full Sat+Sun of January | **30 hours**: 1600 UTC Saturday → 21:59 UTC Sunday |

The WFD date rule accounts for the "last full weekend" requirement: if the last Saturday of January would put Sunday in February, the code steps back one week. 2026 correctly resolves to Jan 24.

### Countdown

The FieldDay header shows a live countdown as the event approaches:

- **starts in N days** / **starts tomorrow** / **starts in Nh** / **starting soon** / **active**

The header reads **active** once the window opens and clears back to countdown after the event window closes (27 hours for ARRL FD, 30 hours for WFD).

---

## Pre-Event Checklist

Run through this before the weekend. Most problems are discovered Saturday at 1759 UTC, not Friday evening.

- [ ] **Set your class and section** in Settings → Contesting ▸ Field Day Setup (e.g. `3A`, `WI`). Both fields start **empty** — the greyed `1D` / `WI` you see are placeholder hints, not values. Field Day mode will not engage until both are filled in (the exchange goes on the air), and an export with blanks is malformed.
- [ ] **Set power multiplier**: x5 (QRP/battery), x2 (≤100 W, the default), or x1 (>100 W). The engine clamps illegal values to the nearest legal tier.
- [ ] **Configure N3FJP** (see [N3FJP Setup](#n3fjp-setup) below) and press **Test** to confirm the handshake before the event.
- [ ] **Configure N1MM address** if your club runs N1MM dashboards (see [N1MM Broadcast](#n1mm-broadcast)).
- [ ] **Verify CAT / PTT** on all bands you plan to use. Use **Settings → Radio ▸ Rig & CAT** and the Test Tone / key-up checks on [Rig and Audio Setup](Rig-and-Audio-Setup.md).
- [ ] **Test the Phone and CW cockpits** end-to-end: make a test QSO on a non-event day to confirm the FD log strip accepts a manual entry and that the dupe toast fires on a repeat.
- [ ] **Claim bonuses** in the bonus checklist once they are achieved during the event (see [Bonus Checklist](#bonus-checklist)).

---

## Exchange and Mode Codes

ARRL FD exchange is **Class + ARRL Section** (e.g. `3A WI`).

Nexus logs three mode classes, matching ARRL's mode-class dupe rule:

| On-air mode | FD mode code | QSO points |
|---|---|---|
| FT8, FT4, TempoFast, TempoDeep | DIG | 2 |
| CW | CW | 2 |
| SSB / Phone | PH | 1 |

The same callsign counts **once per band per mode class**. Working K1ABC on 20 m CW and then 20 m FT8 are two legal contacts (different mode class). Working K1ABC on 20 m FT8 twice is a dupe — the log strip will reject the second attempt with an error toast. Dupe checks are case-insensitive.

---

## Scoring Formula

The live scoreboard shows:

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

WFD scoring in Nexus is partial. QSO points and the bonus checklist are tracked. WFD operator-count and objective multipliers are **not** computed in-app. The UI states *"WFD objective multipliers apply at submission (not tracked here)"* — use the raw point export and apply multipliers in the WACA WFD scoring tool at submission.

### Winter Field Day Mode Rules

The WFD rules ban the entire WSJT-X mode suite (FT8, FT4, FST4, JT4, JT9, JT65, Q65, MSK144, WSPR and friends) while explicitly keeping RTTY and SSTV legal as Digital. Nexus carries this list as advisory rules data — it does **not** block or disable any mode. Staying inside the rules is your call; what Nexus does guarantee is that a digital contact is exported and pushed under the mode actually used (an RTTY contact says RTTY, never FT8), so a legal contact can never be misreported as a banned one.

---

## Scoreboard, Sections Board and Pop-Out

The FieldDay view carries a live scoreboard: QSO and section counts, per-mode chips (DIG / CW / PH), and the score math for the active event (WFD shows honest raw points, never the ARRL power×+bonus formula).

- **Operator field** — Field Day rotates operators; type the call of whoever is at the key. It persists across restarts, and each QSO pushed to N3FJP is attributed to that operator (falling back to the station call when empty).
- **Sections board** — all 83 ARRL/RAC sections laid out division by division, each cell turning green with a ✓ as the section is worked, with a worked/total count. It doubles as your multiplier tracker.
- **Pop out** — the button in the scoreboard header tears the whole scoreboard (operator, tiles, sections board) off into its own window, sized for a second monitor or a club display facing the room. The docked view keeps working independently.
- **Club Board** — the **club band board** (position, band, mode, operator, QSOs, rate) has its own button in the left rail, directly under Field Day, and its own window. It appears whenever Field Day is on, whether or not club sync is running, and one click puts it on a second monitor: this is the board a multi-station club watches all event to see who is on what band before moving to another one. The same **Pop out board** button in the club header on the dashboard opens the same window. It is set in larger type than the docked copy because it is watched from the operating position rather than read at the keyboard, and it is a monitoring window: no operator field and no export buttons, both of which live on the dashboard.
- **With club sync off**, the window says so and names the route that turns it on (Settings ▸ Contesting ▸ Field Day Club Sync ▸ Host a club event) instead of showing an empty board. With sync on and nobody else logging yet, it says it is waiting.

---

## Bonus Checklist

The bonus checklist contains exactly 15 ARRL FD bonuses. Toggle each one in the FieldDay view as your club achieves it:

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
| Web submission | 50 |

**Total possible bonus: 1 450 points.**

The bonus checklist models ARRL FD bonuses only. WFD has a different bonus structure that is not separately modeled.

---

## All-Mode Logging

### Digital (TempoFast auto-sequencer)

When the FD workspace is open and a digital contact is in progress, the TempoFast auto-sequencer handles the 4-step exchange autonomously once you initiate:

- **S&P** (Search-and-Pounce): double-click a CQ decode → sequencer sends your exchange → accepts their roger → logs the QSO.
- **Running**: answer an incoming exchange → roger with your exchange → accept their RR73 → log.

Entering Field Day from the nav always starts in **Search-and-Pounce**. Switch to Running via the button pair in the FieldDay header.

The WSJT-X UDP `Status` message sets `special_op = 3` (Field Day) while FD mode is active. Once the **WSJT-X UDP API** switch in Settings → Logging & Connectors ▸ Integrations & Feeds is on (it is off by default), JTAlert and GridTracker will automatically activate their FD-specific behavior without any other configuration on your end. FD contacts are also emitted as `QsoLogged` UDP datagrams to the same sink.

### CW

Navigate to the CW cockpit. The log strip detects that FD mode is active and shows **Class** and **Section** fields alongside the standard call/RST fields. Fill in the exchange and press Log — the entry routes to `fdLogManual()` with mode code `CW` and is dupe-checked against the FD log.

The CW cockpit pre-fills Class from the most recent FD entry so you do not retype it for every contact.

### Phone

Navigate to the Phone cockpit. The log strip similarly adds Class and Section fields and routes to `fdLogManual()` with mode code `PH` (1 point). RST defaults to 59.

All three mode classes write into the **same unified FD log**, so the live score and Cabrillo export reflect the full multi-mode total in real time.

### Contest Log Persistence

The contest log survives restarts: every logged contact is journaled to `fieldday_backup.adi` (beside `settings.json`), and the journal is restored automatically whenever you re-enter Field Day mode — a mid-event quit, crash, or Run/Search-and-Pounce switch loses nothing. Entries from a previous event (older than 4 days) are not restored, so the journal self-expires between events.

One consequence to know: contacts logged during a **pre-event gear test within 4 days of the event** are restored into the real event's log and dupe sheet. To start the event clean, delete `fieldday_backup.adi` after testing (with Nexus closed, or at least outside Field Day mode — the next contact logged in FD mode re-writes the whole journal from memory).

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

All four exports are available at any time during or after the event from the FieldDay view export buttons.

### Cabrillo 3.0

- Each QSO line carries a real `yyyy-mm-dd hhmm` UTC timestamp derived from the logged Unix timestamp.
- Mode tokens follow Cabrillo 3.0: `CW`, `PH`, `RY` for RTTY contacts, `DG` for other (or unrecorded) digital.
- `CONTEST:` header is `ARRL-FD` or `WFD` based on the event switch. These are Cabrillo names,
  from the WA7BNM Master List of Cabrillo Names that the Cabrillo V3 specification points to;
  they are not always the same string as the ADIF `CONTEST_ID` below (ARRL Field Day's ADIF id
  is `ARRL-FIELD-DAY`).
- `CATEGORY-OPERATOR` is the **Entry category** you pick in Settings → Contesting ▸ Contest (`SINGLE-OP` by default).
- Legacy contacts without a timestamp fall back to the `----------` placeholder rather than inventing a time.

### ADIF

- Tags written per contact: `CALL`, `MODE`, `BAND`, `CONTEST_ID` (ARRL-FIELD-DAY or WFD), `CLASS`, `ARRL_SECT`, `<EOR>`.
- `PROGRAMID` is `Nexus`.
- `MODE` is the mode actually worked: CW maps to `CW`, Phone to `SSB`, and a digital contact carries its real mode (`FT8`, `FT4`, `RTTY`, …). Only legacy digital rows logged before the actual mode was recorded fall back to `FT8`.

### Score Summary

A one-page plain-text score summary: QSO counts by mode and by band, the sections worked, power multiplier, claimed bonuses and the score math (WFD prints raw QSO points and notes that objective multipliers apply at submission). Hand it to the club scorekeeper or check your entry against it before submitting.

### Dupe / Multiplier Sheet

A plain-text check sheet: every section multiplier with the call and band that first earned it, then an alphabetical callsign list showing how many times and where (band/mode) each station was worked, with dupes flagged `*`.

Submit the Cabrillo file to the ARRL online submission system. ADIF can be imported into N3FJP or other loggers for cross-checking.

---

## Limits / Not Yet

- **WFD scoring is partial**: QSO points and bonuses are tracked; WFD operator-count and objective multipliers are not computed in-app.
- **Bonus checklist is ARRL FD only**: WFD has a different bonus structure that is not modeled.
- **N3FJP errors are not surfaced in the UI** beyond the initial Test button; monitor N3FJP's own display to confirm pushes are landing.
- **N1MM is emit-only**: Nexus does not receive inbound `<contactinfo>` from other network stations.
- **Legacy digital rows export as FT8**: contacts journaled before the actual on-air mode was recorded have no mode on file, so ADIF and the interop push fall back to `FT8` for them. New digital contacts carry the mode actually worked.
- **TempoFast auto-sequencer requires operator initiation**: fully unattended automated operation is not implemented, consistent with ARRL FD rules requiring operator presence.
- **Desktop-only** (Tauri v2); no mobile companion.

---

## Other Contests, and CQ WW RTTY

The same workspace runs every contest on the **Settings → Contesting ▸ Contest** picker, not just the two Field Days: Sweepstakes, the ARRL VHF contests, CQ World-Wide DX and WPX, the CQ World-Wide RTTY DX Contest and five state QSO parties. Pick one, then turn on **Field Day mode** in Field Day Setup, which is the switch for every contest. The log strip, the contest screen and the exports then follow that contest's rules. Class, section, the power tiers and the bonus checklist belong to Field Day and stay out of the way.

**CQ WW RTTY** (last full weekend of September, 0000Z Saturday for 48 hours):

- **Before the weekend**, set your **CQ zone** and your **State or province** under Settings → Contesting ▸ Your station data. Stations in the continental USA and Canada send `599`, their zone and their state or Canadian call area, using the sponsor's own codes (`NF`, `LB`, `NWT` and `PEI` among them; `NT` and `PE` typed there are read as `NWT` and `PEI`); everyone else sends `599` and their zone. If your callsign is in the US or Canada but the state is blank or not on the sponsor's list, you would send the DX exchange with no QTH: Nexus warns you in Settings, on the log strip and when the contest starts, and suggests a code where a section name points to one. It is a warning only, because operating from outside the US and Canada really is DX. The Contest section shows the exchange you are about to send. Set **Power category** and **Spotting assistance** under Contest too, because the Cabrillo header declares them.
- **The log strip** asks for RST (599 by default), the zone and the QTH. The QTH can stay blank for a DX station. Type a call and the zone box shows that call's usual CQ zone from the country file as a faint hint; it never fills the box for you. A USA or Canada station logged without a QTH gets a warning, not a refusal. A QTH that is not on the sponsor's list (an ARRL section like `EMA`, a typo) still logs, but it counts as no multiplier. If the rig is on a band the contest does not use (it runs on 80, 40, 20, 15 and 10 m), the strip says so and still logs the contact.
- **Scoring** follows the sponsor: 3 points between continents, 2 between countries on the same continent, 1 within your own country. Zones, countries and W/VE QTHs each count once per band, and the USA and Canada count as countries. Each station can be worked once per band.
- **RTTY Auto** does not run in this contest. Send your exchange with the macros and log each contact in the strip.
- **The Cabrillo export** follows the sponsor's template: `CONTEST: CQ-WW-RTTY`, the dial you were on when you logged each contact, a two-digit zone, and `DX` in the QTH column where a station sent none. The headers carry CATEGORY-ASSISTED, -BAND, -MODE and -POWER, CLAIMED-SCORE, your operator name as NAME, and the **Email for contest logs** setting as EMAIL (left out when blank). LOCATION is your state or province in the sponsor's LOCATION spelling (`PEI` becomes `PE`, for example), or `DX`. A log with contacts on one band is declared single band, as the sponsor classifies it. If you entered one band but also logged others, change CATEGORY-BAND by hand before you upload.

**Illinois QSO Party** (1700Z Sunday of the third full weekend of October, for eight hours):

- **Before the party**, set your **State or province** under Settings → Contesting ▸ Your station data, and your **County** as well if you are in Illinois. Illinois stations send `599` and their county; everyone else sends `599` and their state, province or **country** — this party asks DX stations for their country rather than the word `DX`, so type the country into the state box. The Contest section shows the exchange you are about to send.
- **The log strip** asks for RST and one QTH box, and that box takes the **county code or the county name**: type `Cook` and it offers `COOK`, `st clair` offers `SCLA`. Press space and the name becomes the code that goes in the log. A **half-typed name is not completed for you** and neither is one that could be several counties — `Ma` is Macon, Macoupin, Madison, Marion, Marshall, Mason and Massac — so pick from the list or finish typing. Nothing is ever guessed onto the air: the same rule is why a state name that several ARRL sections cover (New York, California) is never turned into one of them in Field Day's section box. A state, a province or a country typed there logs as it is.
- **Bands**: 160 through 2 metres, **without the WARC bands** (the sponsor's own list excludes 60, 30, 17 and 12 m). If the rig is elsewhere the strip says so and still logs the contact.
- **FT8 and FT4 earn no credit at all** — the sponsor's own rule, because of what an ILQP log entry has to contain. Other digital modes are encouraged; RTTY and PSK are ordinary digital contacts here. Nexus warns you and still logs.
- **A station counts once per band and mode, and CW and digital are ONE mode here**: work somebody on CW and the strip shows them as a dupe on RTTY on that band. Phone is separate. An Illinois mobile or rover in a new county is a new contact either way. A station **on a county line counts once per county** — log it once per county, one contact each, which the dupe rule allows. **Repeater contacts do not count.**
- **Scoring**: phone 1 point, CW and digital 2. Illinois stations multiply by Illinois counties plus US states, Canadian provinces and up to **five** DXCC entities (Canada, Hawaii and Alaska are not DX entities here); everyone else multiplies by the Illinois counties worked. The sponsoring club's two calls, **W9AWE** and **W9OAB**, are worth **100 bonus points each, once per log**, and Nexus adds them to your score and to the Cabrillo claimed score as soon as they are in the log — there is no box to tick.
- **The Cabrillo export** writes `CONTEST: ILLINOIS QSO PARTY`, which is what the sponsor's own sample log generates. Other loggers write `IL-QSO-PARTY` and the club plainly accepts those too, so change it if you prefer. An Illinois entry also gets `IL-COUNTY:` with your county's **name**, beside QSO lines carrying its four-letter code, exactly as the sample log does. The sample's `ENTRY-CLASS`, `QRP-COMPETITION`, `CLUB`, `OPERATORS` and address headers are **not** written: Nexus has nothing to fill them from, and a header nobody set is a claim nobody made — add them by hand if your entry needs them. The ADIF export uses ADIF's own contest name, `IL QSO Party`.
- **The log deadline is not in the rules Nexus read** for 2026 (2025's was midnight Central on 5 November). Check the sponsor's page before you send your entry.

---

*Previous: [Operating Guide](Operate-FT8-FT4.md) — Next: [Rig and Audio Setup](Rig-and-Audio-Setup.md)*
