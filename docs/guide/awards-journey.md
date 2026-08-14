# Awards & Journey

This section has two tabs under one roof: **Official Awards**, the offline
tracker for the official programs (DXCC, Challenge, Honor Roll, WAS, WAZ, VUCC,
IOTA), and **Journey**, a local-only achievement layer that turns your log into
progress you can feel — firsts, ladders, collections, and personal bests. Both
read straight from your [logbook](logbook-qsl.md); neither needs an account or a
network.

Awards is an opt-in section, nudged on after your first QSO. Turn it on any time
in [Settings ▸ Appearance ▸ Features](settings-reference.md#features).

![The Official Awards tab, its ten award cards wrapped onto two rows: DXCC 233/100, Honor Roll 233/340, Challenge, a 53% confirmed rate, 5-Band DXCC and WAZ across the top, then WAS, VUCC, Sat VUCC and IOTA below — WAS and VUCC outlined as achieved, Sat VUCC still 0 of 100 with one grid worked via satellite. Under the cards a DXCC-by-band chart runs 160 m down to 6 m beside a "Confirm for a new one (46)" list, each entity chipped with the bands it is waiting on a confirmation for.](../img/manual/awards-official.webp)

## Awards — the official programs

Awards are **computed offline** from cty.dat entity resolution — no upload
required to *see* where you stand:

- **DXCC** with per-band and per-mode slots, and **5-Band DXCC**,
- **DXCC Challenge**,
- **Honor Roll**, with a current-entity denominator and a countdown —
  confirmations still needed to get in, then "N to #1" once you're in,
- **WAS** (Worked All States), with the 5BWAS count beside it,
- **WAZ** (Worked All Zones),
- **VUCC**, with satellite grids kept on their own **Sat VUCC** card — Nexus
  doesn't tag a satellite contact as one, so that card only moves for records
  you tagged yourself ([Satellites](satellites.md) explains why),
- **IOTA** (Islands On The Air).

Under the cards, DXCC is broken out **by band**, and the chase lists start with
**Confirm for a new one** — entities you have worked but not confirmed, chipped
with the bands each is waiting on.

**Confirmation handling is source-aware** — a distinction most loggers blur.
**eQSL confirmations never count toward LoTW-grade awards**; the `confirmed` and
`award_confirmed` flags are separate and enforced at every computation. So a
contact can be "confirmed" (you have an eQSL) yet not "award-confirmed" (no LoTW
or card), and the awards math respects the difference. The
[logbook diagnostics](logbook-qsl.md#understand-why-a-contact-isnt-confirmed)
explain per-QSO why a credit hasn't landed.

## Journey — progress you can feel

Journey exists to carry a newer operator through the motivational dead zone
between QSO 1 and QSO 100 — but it credits an imported logbook immediately, so a
veteran importing years of ADIF sees their history light up at once. It's
**local-only**: no accounts, no network, no decaying daily streaks.

<!-- TODO: capture screenshot — the Journey tab — XP/level hero card, Firsts, ladders, feats -->

What's on the board:

- **XP and levels**, with a hero card.
- **Firsts** — auto-detected milestones ("first DX," "first CW," "first park"),
  each named with heritage context.
- **Ladders** — tiered progress toward the official awards, plus:
  - **DX Marathon** — entities and zones worked this calendar year vs. your best
    year (resets Jan 1),
  - **Grid Gems** — rare/ultra-rare grids worked (rungs at 1/5/10/25/50),
  - **Park Hunter** and **Summit Chaser** — distinct POTA/SOTA references hunted
    (they appear after your first hunt),
- **Seasonal feats** — earned in a season, permanent once earned, with no FOMO:
  - **Sporadic-E Summer** (6 m ≥ 1000 km in season),
  - **Top-Band Season** (160 m ≥ 1500 km).
- **Collections and personal bests.**

Some feats read your station power (from
[Settings ▸ Digital](settings-reference.md#digital-ft8ft4)) — set it to
unlock the miles-per-watt and QRP feats.

### Optional weekly streak

Off by default: a gentle **"weeks on the air"** counter (never a daily streak,
never a penalty for a break). Enable it under Journey in
[Settings ▸ Digital](settings-reference.md#digital-ft8ft4).

### Share a card

The **⤴ Share** control renders the hero card (and any unlocked feat) to a PNG
**on your clipboard** — nothing is uploaded. Click it, then paste into a
message, an email, or a post.

## Honest limits

- **Everything here is local.** Journey never leaves your machine — no
  leaderboards, no accounts, no telemetry.
- **Awards are as complete as your log and its confirmations.** Offline
  computation shows worked/confirmed from what you have; pull confirmations with
  the [LoTW / eQSL sync](logbook-qsl.md#upload-to-lotw) to fill in the
  award-confirmed column.
- **eQSL and HRDLog.net don't earn ARRL award credit** — Nexus won't pretend they
  do.

## Related guides

- [Logbook & QSL](logbook-qsl.md)
- [Stats](stats.md) — the same log counted rather than judged against a list
- [Needed — DX that's on the air now](needed-dx.md)
- [Contesting & POTA/SOTA](contesting-pota.md)
- [Settings reference](settings-reference.md)
