# Spots

Spots is the raw feed: every DX-cluster and Reverse Beacon Network spot Nexus has
taken in over the last twenty minutes, in one table, unranked. It is the "what's
on the air" list — the [Needed board](needed-dx.md) is the "what should I work"
list. Nothing here is scored against your log, nothing is hidden because you
worked it in 1998, and nothing carries an evidence line; you get the report as it
arrived, a column to sort it by, and the same single-click work-it path the
Needed board has.

Spots is an opt-in section: the first-run Goals step turns it on under **DX
chasing & awards**, **Contesting** and **Everything (expert)**, and
[Settings ▸ Features](settings-reference.md#features) turns it on or off any
time. It describes itself there as "Every cluster/RBN spot on the air — the raw
firehose, filter by band/mode."

<!-- TODO: capture screenshot — the Spots board on a busy evening: a few hundred rows sorted by Age, the filter drawer open with band chips and mode chips visible, and the header count reading a filtered "N of M" -->

## The tour

**The header** holds the row count and, whenever anything is narrowing the list, a
second count reading "of *N*" — so you always know how much of the firehose you
are looking at. Beside it sit the hint "every spot on the air — single-click to
work it", the search box, and the **Filter** button, which reads **Filtered**
while any chip filter is set.

**What feeds it.** Two RBN skimmer firehoses — CW and digital — plus *every* human
DX-cluster node you list in
[Settings ▸ Connections](settings-reference.md#connections), unioned into one
buffer. RBN carries CW and digital only, so phone rows come from the human nodes:
with no node configured this board is nearly all CW and FT8. PSK Reporter does
**not** feed it — that is the Needed board's evidence lane and the propagation
nowcast's — and neither do your own radio's decodes. The panel re-reads the buffer
every 15 seconds, so a spot is on screen within a quarter-minute of the feed
carrying it, and it ages out of the buffer at 20 minutes (the Needed board's
window is 15, so Spots holds a station about five minutes longer).

**One row per station per frequency.** A second report of the same call within
2 kHz replaces the row and resets its age rather than adding another line, so a
station being spotted every thirty seconds by six skimmers stays one row that
stays fresh instead of filling the screen.

**The columns**, left to right:

- **Age** — compact: seconds under a minute, then minutes, then hours.
- **Call** — a button, not text. Clicking it opens that station's QRZ.com page in
  your browser and does *not* work the spot.
- **Entity** — the DXCC entity resolved from the callsign; `—` when it doesn't
  resolve.
- **Band** — the band-plan label, `—` when the frequency is off the plan.
- **Freq** — MHz to three decimals.
- **Mode** — the specific mode when the RBN wire carried one (FT8, FT4, RTTY, PSK,
  CW), otherwise the class Nexus derives from the frequency: CW, Phone or Digital.
  Hovering says which you are looking at ("FT8 spot (Digital)"). A human node's
  free-text mode is never read as a mode.
- **Spotter** — who reported it most recently.
- **Comment** — the spot's comment text, verbatim.

**Sorting.** Six of the eight headings are buttons — Age, Call, Entity, Band,
Freq, Mode. Click to sort, click the same one again to reverse it; an arrow marks
the active column. Band sorts by frequency, because a band column ordered by
frequency reads the way a band map does. Ties break newest-first. Spotter and
Comment don't sort.

**The filter drawer** opens from the Filter button and holds four things:

- **Band chips** — 160 m through 6 m always, plus any other band present in the
  current spots. Selected chips mean *show only these*.
- **Mode chips** — the specific modes actually in the feed right now, in operating
  order (CW, Phone, then the digital submodes). These work the other way round:
  everything is shown by default and clicking a chip *hides* that mode ("Hide FT4
  spots"). Nexus stores what you hid, not what you kept, so a mode that first
  turns up mid-session shows rather than arriving silently hidden.
- **US state chips** — the states resolved for the spots in hand (from the FCC
  callsign→state index, refined by a cached grid for a station you have heard
  before). Selected chips mean *show only these*, and a spot whose state didn't
  resolve drops out while any state chip is on.
- **My privileges** — "Show only spots you may transmit to under your license
  class ([Settings ▸ license](settings-reference.md#station)). Open class sees
  everything either way." The flag is computed from the same tables as the
  transmit lockout, so what survives this filter is exactly what you can key on.
- **Clear** wipes the chips and the privileges toggle. It does not touch the
  search box.

While any chip filter is set the drawer stays on screen — you can't end up
looking at a short list with no visible sign of what is shortening it.

**Search** narrows on space-separated terms that AND together, each term matching
any field: call, entity, spotter, mode, band or frequency. So `w1 20m cw` is
W1-callsigns spotted on 20 m CW. `Esc` or the ✕ clears it. The Comment column is
displayed but not searched.

Filters, search text, sort column and the drawer's open state all survive leaving
the section and coming back — they are held for the life of the app run and go
when you quit.

## Core workflows

### Work a spot

1. Click the row anywhere except the callsign. Nexus QSYs band, mode and exact
   frequency in one step and opens the cockpit matching the spot's mode class:
   CW → CW, Phone → Phone, everything else → the digital cockpit.
2. On an FT8 or FT4 row the tier switches first, then the QSY to the spot's exact
   frequency wins over the tier's default dial — click an FT4 spot and the decoder
   is on FT4, not left on FT8.
3. If the freshest comment for that call names a listening offset — `UP 2`,
   `DN 1.5`, `QSX 7.205` — rig split is pre-set so your transmit lands where the DX
   is listening.
4. On a CW or Phone row the callsign is prefilled in the log strip; a toast
   confirms "▶ *CALL* — *mode band*, ready to log".
5. If the matching cockpit is switched off (CW, Phone and RTTY are all opt-in),
   the rig still QSYs and you stay where you are rather than being dropped on a
   hidden view.

An RTTY row opens the **digital** cockpit at the RTTY frequency, not the RTTY
cockpit — only FT8 and FT4 route by submode; everything else routes by the
frequency-derived class. The row's tooltip offers "Work *CALL* …" only when the
spot's band is one your band plan knows; on any other band it just names the spot,
frequency and spotter.

<!-- TODO: capture screenshot — the filter drawer open with "My privileges" active and two band chips selected, the header reading "Filtered", and a row hover showing the "Work W1AW — CW @ 14.025 MHz (spotted by K3LR)" tooltip -->

### Cut the firehose down to what you can work

1. Open **Filter** and turn on **My privileges** — everything you may not transmit
   to disappears. (An Open-class, non-US operator has every spot licensed, so the
   toggle changes nothing.)
2. Select the band chips you have an antenna for.
3. Hide the modes you don't operate — one click per mode chip.

What's left is a working list, and it stays that way while you move around the app.

### Chase a state for WAS

Open **Filter**, pick the state chips you still need, and sort by Age. Only spots
whose state Nexus could resolve remain — a cluster spot of a station it can't place
is not shown as "maybe", it's gone while the filter is on.

### Find one station in a contest weekend

Type into the search box. A call fragment, an entity, a spotter's call or a
frequency all match, and terms stack: `ja 15m` is Japan on 15 metres. When the
board is empty it says which case you're in — "No spots match the current filters
— clear to see all", or "No spots yet — cluster/RBN spots appear here as they
arrive."

## Honest limits

- **No ranking and no log awareness.** No need chips, no ATNO colouring, no
  worked-before marking, no evidence line, and no dedupe against your log — a row
  is a report, not an opportunity. Everything that judges a station's value to
  *your* log lives on the [Needed board](needed-dx.md).
- **Cluster and RBN only.** PSK Reporter reception reports and your own radio's
  decodes never appear here, however good the path they prove. And since RBN
  skimmers only spot CW and digital, phone coverage is exactly as wide as the
  human cluster nodes you have configured.
- **The Mode column is a judgement about frequency**, except where an RBN skimmer
  named the mode. A DX SSB station parked below the US phone edge on 40 or 80 m
  (7.085, say) falls in the data window and reads Digital. That is deliberate: the
  free-text mode token in a spot comment is unreliable everywhere else, so the
  number on the dial is trusted instead.
- **Beacons and bulletins are not badged on this board.** Nexus does identify
  NCDXF/IARU beacon slots and W1AW bulletin frequencies, and badges them in the
  Phone and CW cockpits' Band Activity strip — this table shows them as ordinary
  rows, so a click can QSY you to a one-way transmission that will never come back
  to you.
- **One spotter per row.** The other stations that reported the same DX are folded
  into the row behind the scenes and not displayed; CQ zone isn't shown either, and
  the resolved US state is filter-only — it never gets a column.
- **Filters are for this run of the app.** They survive leaving the section and
  coming back, and they are gone at exit — unlike the Needed board's, which persist
  across restarts.
- **Spots doesn't tear off.** There is no ⧉ pop-out for this board; the Needed
  board, Connect and the cockpit band maps detach, this one doesn't.
- **Twenty minutes, and nothing before that.** No history, no export, no "what did
  I miss while I was away".
- **Declaring an unassisted entry empties the board.** The switch stops cluster/RBN
  ingestion *and* the read returns nothing, so a spot that arrived a second before
  you flipped it is invisible immediately rather than lingering until the buffer
  drains.

## Related guides

- [Needed — DX that's on the air now](needed-dx.md)
- [Connect — map + propagation](connect.md)
- [DXpeditions](dxpeditions.md)
- [Settings reference](settings-reference.md)
