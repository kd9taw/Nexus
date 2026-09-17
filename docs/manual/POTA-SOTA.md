# POTA/SOTA

Nexus surfaces live Parks On The Air and Summits On The Air activators, annotates each spot with what your log and your signal say about it, and tags your logbook with the correct ADIF fields — from a single HUNT click. It also runs your own activation: park-tagging every contact you log and spotting you on pota.app and the DX cluster when you ask.

## Hunting and activating

The board is built around hunting, with your own activation in a strip above the list:

- **Hunting** — the spot list, filters and the HUNT button, described below.
- **I'm activating** — pick POTA or SOTA, type your reference (`K-1234`, `W7A/MN-001`) and press **Start**. Every contact you log while the activation runs is stamped with your park (`MY_SIG`/`MY_SIG_INFO` or `MY_SOTA_REF`), and the strip counts them. **Stop** ends the activation; it never stops a transmission.
- **Spot me** (shown while you are activating) posts your park and dial frequency to pota.app and to the DX cluster — only when you press it, and only after you confirm the park and frequency it will send. If either changes between the press and the post, nothing is sent.
- **Park list** — download the POTA park directory once, or import a CSV, and search it offline from the log form.
- **Import Hunted Parks** — load the `Hunted Parks.CSV` from your POTA stats page, so parks you hunted on CW (where the park reference is not in the exchange and your log cannot know it) stop showing as NEW PARK.

The board can be popped out into its own window (**⧉ Pop out**) — a POTA board beside a SOTA board, each keeping its own program, filters and sort.

## Spot feeds and poll cadence

Two feeds are supported, selectable via program toggle chips at the top of the view:

| Program | Source | Endpoint |
|---------|--------|----------|
| POTA | pota.app public API | `https://api.pota.app/spot/activator` |
| SOTA | SOTAwatch | `https://api-db2.sota.org.uk/api/spots/30/all` |

All HTTP requests carry a `nexus-pota/0.1 (+ham radio parks/summits on the air)` User-Agent and a 15-second timeout. No credentials are required for either feed.

**Poll cadence:** the view auto-refreshes every **60 seconds**. A manual **Refresh** button is always available. After each successful fetch, a `HH:MM:SS` last-updated timestamp appears next to the Refresh button so you know exactly how stale the data is.

When you log a contact (or anything else changes your log), the board updates its badges and its hidden rows from the spots it already has — it does **not** fetch the feeds again for that, because your log also changes each time an upload to QRZ, ClubLog or LoTW is recorded.

Selecting **Both** fetches POTA and SOTA concurrently (parallel HTTP) and merges the results into a single sorted list.

> **SOTA cap:** the SOTA fetch is hard-coded to the **30 most recent spots**. On a busy summit day with more than 30 simultaneous activators, older spots will not appear. There is no workaround short of checking SOTAwatch directly.

## Filters

Band and mode filter chips appear dynamically based on what is present in the current spot list:

- **Band chips** follow ITU Region 2 order (160 m through 2 m); multi-select is supported. Only bands represented in the live spot list are shown.
- **Mode chips** — All / SSB / CW / FT8 / FT4 / OTHER — appear in that preferred display order; only modes in the live list are shown.

Select multiple band or mode chips to combine filters. Program, filters and sort are remembered per board window.

## Hide worked today

**On by default.** An activator you have already logged **at the park they are spotted at, since 0000Z (UTC)**, is hidden: you have that activation, so the board lists only the ones you still need today. The chip beside the program tabs says how many it is hiding — **Hide worked today · 3**.

- **It comes back on its own** at 0000Z, when a new activation day starts, or as soon as that activator is spotted at a **different park**.
- **Only a contact logged with the park counts.** HUNT, double-clicking the park on the Connect map, and **Work** on the Needed board all tag the next contact with that activator for you. A contact logged without the park reference hides nothing — that is the safe direction, but it can look as if the chip is not working. Any band or mode counts; the activator's portable suffix (`K1ABC/P`) does not matter.
- **To see every activator again**, click the chip. Hidden rows come back marked **WORKED TODAY**.
- If every activator on the board is one you have worked, the list says so and points at the chip rather than showing an empty board.
- 0000Z is decided by the station's own clock, not the time zone your computer is set to.

A contact logged with two park references (a two-fer, `US-0001,US-0002`) counts for both parks.

## Spot cards

Each spot card shows:

- Activator callsign
- Park or summit reference (e.g. `K-1234`, `W4C/EM-023`)
- Park name (truncated at 28 characters)
- Frequency to 4 decimal places (10 Hz resolution)
- Band and mode
- **NEW PARK**, **BAND OPEN** and **WORKED TODAY** badges where applicable (see below)

Spot age from the API (`spotTime`) is fetched but not displayed per-card; the last-polled wall-clock timestamp is the freshness indicator.

## NEW PARK badge

A **NEW PARK** badge appears when the park or summit reference has never appeared on the hunter side of your logbook (the `SIG_INFO` or `SOTA_REF` field for your own contacts) **and** is not in the Hunted Parks CSV you imported. The lookup is case-insensitive and runs against the full local logbook — no external API call, no manual tracking.

If you have previously worked that park on any band or mode, the badge is absent even if this is a new band slot. NEW PARK means the reference itself has never been in your log.

## BAND OPEN badge

A **BAND OPEN** badge appears when PSK Reporter reception reports confirm that your own signal has been heard on that band within the **last 15 minutes** (900 seconds). It is a propagation gate, not an estimate: it requires the live PSK Reporter MQTT feed to be active and your callsign to have been reported recently by a receiver on that band.

If you have not transmitted on a band in the past 15 minutes, or if the PSK Reporter feed is not connected, the badge will not fire — there is no fallback guess.

## Sort order

The default sort, **Workable now**, puts the most actionable rows first:

1. **BAND OPEN** spots (score 2) — the band is demonstrably open from your QTH right now
2. **NEW PARK** spots (score 1) — a reference you have never worked
3. All other spots — in API order

A row that is both BAND OPEN and NEW PARK is the highest-priority contact in the list. The sort picker also orders by activator, reference, band/frequency or mode, and the arrow flips the direction.

## HUNT flow

Clicking **HUNT** on a spot card does three things atomically:

1. Validates and normalizes the park or summit reference:
   - POTA: requires a 1–4 character alphanumeric prefix, a hyphen, and 4–5 digits (e.g. `K-1234` or `US-12345`)
   - SOTA: requires `association/region-NNN` format (e.g. `W4C/EM-023`)
2. Records a **pending hunt** tagged with the current Unix timestamp and the activator's base call
3. QSYs your radio to the spot's exact frequency and opens the matching cockpit (Digital for FT8/FT4, CW, or Phone). For CW and Phone cockpits the activator's callsign is prefilled in the log strip. The Digital (FT8/FT4) cockpit receives no callsign prefill — double-click a decode to start the QSO as normal

An active-hunt banner appears at the top of the hunter view showing the program, reference, and activator call. Clear it with the **X** button if you decide not to work the contact.

The same pending hunt is recorded when you double-click a park on the Connect map, or click **Work** on a POTA/SOTA row of the Needed board.

### Pending-hunt TTL

The pending hunt expires after exactly **4 hours** (14 400 seconds). If you click HUNT on K1ABC/K-1234 and do not log a matching QSO within 4 hours, the pending tag is silently discarded and no park reference is applied to any subsequent contact. This prevents a stale hunt from tagging an unrelated future QSO with the same callsign.

### Base-call matching

The pending hunt matches the activator using **base-call comparison**: portable suffixes like `/P`, `/4`, or prefix affixes are stripped before comparison. A spot for `K1ABC` will correctly tag a logged QSO with `K1ABC/P`, and vice versa.

The tag fires only on the **first** matching QSO, then clears. A non-matching QSO logged before the activator contact does not consume or inherit the pending hunt.

## ADIF serialization

When a QSO is tagged with a pending hunt, the correct ADIF fields are written:

| Program | ADIF fields written |
|---------|-------------------|
| POTA | `SIG=POTA`, `SIG_INFO=<reference>` |
| SOTA | `SOTA_REF=<reference>` |

These are the fields accepted by pota.app upload and the SOTA database import. No manual ADIF editing is needed.

## Needed-board integration

The **Needed board** automatically injects a **POTA** or **SOTA** chip onto any row whose callsign matches a live activator in the cache — provided the cache is no older than **10 minutes** (600 seconds). The activator cache is warmed by a background poller from app launch on a ~3-minute cadence (both POTA and SOTA), so the chips — and standalone park/summit chase rows — appear on the Needed board without opening this view first.

A park or summit you have **not** worked in the activation running now also earns a **New park** need, which ranks the row above a mere confirmation and below every DX award. Once you have logged it today, the row keeps only its POTA/SOTA label and sinks below the real needs; the same park tomorrow is a new opportunity again. Use the POTA/SOTA filter chips on the Needed board to isolate these rows.

Clicking **Work** on one of these rows records a pending hunt for that park before it QSYs, so the contact you make is logged with the park — exactly as HUNT does.

## Journey achievements

Your **first logged POTA hunter contact** fires the *First POTA Contact* milestone. Beyond that, two ladders count what you have hunted, and each appears only once you have hunted one: **Park Hunter** (distinct POTA references, from First Park at 10 to Park Legend at 250) and **Summit Chaser** (distinct SOTA summits, First Summits at 5 to Summit Legend at 100). Both count a reference once, however many times you work it.

## Limits / not yet

- **SOTA 30-spot cap.** The SOTAwatch fetch is hard-coded to 30 spots. Busy summit days may omit older activators.
- **No offline mode.** All fetches are live HTTP. A network outage returns an error toast and an empty list; there is no cached fallback.
- **BAND OPEN requires recent TX.** Without an active PSK Reporter MQTT feed or recent transmission on the target band, the badge never fires.
- **Hide worked today needs the park on the contact.** A contact logged without the park reference never hides its activation.
- **Remote:** a browser watching the station through Nexus Remote shows the board without Hide worked today.
- **Spot age not displayed per-card.** The `spotTime` field is fetched but not parsed or shown; use the wall-clock last-updated timestamp as your freshness reference.
- **No park-name search on the board.** You cannot search the spot list by park name or reference to check whether one particular park is on the air. (The park *directory* is searchable from the log form once you have downloaded it.)

---

Related pages: [Needed Board](Needed-and-Hunting.md) · [Logbook and Awards](Logbook-and-Awards.md) · [Getting Started](Getting-Started.md) · [Rig and Audio Setup](Rig-and-Audio-Setup.md)
