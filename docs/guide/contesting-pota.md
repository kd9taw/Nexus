# Contesting & POTA/SOTA

Two portable/event workflows live here: **Field Day** (ARRL or Winter Field Day),
which reshapes the app for the weekend and pushes to the club's master log in
real time, and the **POTA/SOTA hunter**, which finds activators and tags your
contact for upload. Both are opt-in sections — enable them in the first-run
wizard or [Settings ▸ Appearance ▸ Features](settings-reference.md#features).

---

## Field Day

A settings switch turns the event on and the app reshapes for it: the exchange
grammar, a live countdown that knows the real date rules, per-(call, band,
mode-class) dupe checking, and a scoreboard.

<!-- TODO: capture screenshot — Field Day mode — exchange entry, countdown, live scoreboard -->

### Set it up first

In [Settings ▸ Contesting ▸ Field Day Setup](settings-reference.md#field-day-setup):

1. **Event** — ARRL Field Day or Winter Field Day. This changes scoring labels
   and export headers.
2. **Class / Category** and **ARRL Section** — these start **empty on purpose**,
   and Field Day refuses to start until you set yours. (An old default of "WI"
   sent the wrong exchange for everyone outside Wisconsin — now it's a one-time
   deliberate step.) ARRL FD wants a class like `1D`; WFD wants a category like
   `2O`.
3. **Power multiplier** — ×5 (QRP/battery ≤ 5 W), ×2 (≤ 100 W), or ×1 (> 100 W).
   It multiplies your QSO points; the engine clamps it to the legal values.

### The Field Day cockpit

During a running event, Field Day opens on the **cockpit** — one screen built for
logging and nothing else. The **Cockpit / Dashboard** button in the header swaps
between it and the full Field Day page (setup, bonuses, exports, the club board);
outside an event Nexus opens on the dashboard instead, so there is nothing to
configure either way.

The callsign box owns the keyboard. Type anywhere on the screen and the
characters land in it, so a call you started typing cannot disappear because you
clicked a board a moment earlier. **Enter** logs the contact and leaves you ready
for the next one.

The dupe verdict has a slot that is always there, whether or not it has anything
to say — the entry row cannot jump under your fingers at the moment a warning
appears. Your own log's dupes refuse; another position's are a warning, the way
N3FJP treats them.

The band-and-mode grid answers the question you are actually asking. As you type
a call, it paints: **red** on the cells where you have already worked him,
**amber** where another position in the club has. So "he's a dupe" becomes "he's
a dupe *here* — work him on 15 phone." The sections checklist beside it lights up
when the section you just typed is one nobody has yet. **That checklist counts
this position's own log** — the club's combined sections are on the dashboard and
on the spectator board.

The pane on the left follows the radio: decodes on digital, your keyer macros on
CW, push-to-talk on phone. The mode chip in the header shows what Nexus thinks
the rig is doing and **you can override it** — a misread mode would otherwise
label every dupe check and every logged contact wrong.

**Stop TX sits in the header**, outside the panes, and the cockpit has no panes
you can hide. There is nothing to fumble away at 2 AM and no menu to find it in
again.

### Operate the event

Field Day is **all-mode**: once you initiate a contact, the digital sequencer
runs the FD exchange autonomously, and the [CW](cw.md) and [Phone](phone.md)
cockpits' log strips become FD entries with class/section and **shared dupe
checking** — one laptop covers the whole operation.

The one strip that does **not** switch is the log strip in the
[Satellites](satellites.md) section: it is not wired to Field Day yet, so a
contact typed there during FD goes into your general log and scores the club
nothing. Log satellite contacts from the CW or Phone cockpit **while you are
still on the bird** — the FD log stamps each contact with the band the radio is
on at the moment you type it, so one entered after you have QSY'd away files on
the wrong band, both in the Cabrillo and on the N1MM / N3FJP wire.

The scoreboard shows its work: QSO points (phone 1, CW/digital 2) × the legal
power multiplier + a 15-item ARRL bonus checklist = total. **Winter Field Day
deliberately shows raw counts only** — its objectives math isn't ARRL's, and
Nexus won't display a fake total.

Opening **Bonuses** does not cost you the sections board. The board is the only
part of the Field Day column that can give height, so it used to give it for
everything else and collapse to a blank strip when the fifteen-row checklist
opened. It carries a floor now and the column scrolls past that floor, so the
board a club watches all weekend stays a board. The checklist keeps its own cap
and scrolls inside itself: all fifteen rows are there, in a list of their own,
rather than 290 px of checkboxes between you and the log.

### Export and club interop

Exports are submittable: **Cabrillo 3.0** with real per-QSO UTC timestamps and
per-row mode tokens, plus **ADIF** with `CONTEST_ID`.

The club story is native:

- Every FD contact pushes in real time to **N3FJP** over its official TCP API
  (default port 1100). Configure the master log's host/port and use the **Test
  N3FJP** button at the site before the event.
- Nexus also broadcasts the native **N1MM+** `<contactinfo>` UDP datagram for
  N1MM-networked dashboards.

Both are fire-and-forget on background threads, so a hung logging PC can never
stall your TX slot. The WSJT-X UDP Status message sets `special_op = Field Day`,
so JTAlert/GridTracker auto-activate their FD behavior too.

Configure [N3FJP](settings-reference.md#n3fjp-integration-club-master-log) and
[N1MM+](settings-reference.md#n1mm-integration) in Settings ▸ Logging & Connectors.

### Run the whole club on Nexus (club sync)

If every position runs Nexus, you don't need a third-party master log at all.
One PC at the site turns on **Settings ▸ Contesting ▸ Field Day Club Sync ▸
Host a club event**; every other position presses **Find club events** (or
types the host's address) and joins. From then on:

- Each logged contact streams to the host the moment it lands; the host merges
  everything into one club log and pushes the club totals back.
- Every position gets a **club dupe warning while typing** — if another tent
  already worked that call on this band and mode, you're told before you call.
  It's a warning, not a lock (N3FJP semantics); your own log's dupes still
  refuse.
- A live **band board** shows where every position is (band, mode, operator,
  rate), stale-marked the moment one goes quiet. **Pop out board** puts it in
  its own window, for a second monitor or a corner of the big one — the same
  board the club watches all event, without the rest of the dashboard around
  it. The web scoreboard below carries the same band and mode per position, so
  the screen facing the room answers "who's on 20?" too.
- The sync chip tells the truth: **Synced**, **Behind n**, or **Offline** —
  contacts logged offline are journaled and re-sent automatically on
  reconnect. If the host PC dies, enable hosting on any other position;
  everyone re-joins and nothing is lost.
- The host exports the merged **club Cabrillo / ADIF**, deduplicated the way
  the rules score it (earliest contact wins).

Hosting is the one time Nexus listens beyond the local computer, and only
while the toggle is on. There is no join password — a club site LAN is
trusted; the connection can only carry log rows, never key a transmitter or
change a setting. The N3FJP/N1MM pushes above keep working alongside if you
want both.

---

## POTA / SOTA hunter

The hunter is for **finding activators, not running activations**. It polls the
official feeds (pota.app and SOTAwatch) every 60 s.

<!-- TODO: capture screenshot — the POTA/SOTA hunter — spot list with NEW PARK and BAND OPEN badges -->

### The tour

Live spots with program toggles (**POTA / SOTA / Both**), band and mode filter
chips, park names, and two ranking badges:

- **NEW PARK** — the reference has never appeared in your log (computed from your
  own ADIF, not an external tracker),
- **BAND OPEN** — PSK Reporter confirms your signal is reaching that band within
  the last 15 minutes.

### Hunt an activator

1. Click **HUNT** on a spot. Nexus atomically registers the park as a pending
   hunt target, QSYs to the spot's frequency and mode, and opens the right
   cockpit.
2. Work the activator. The **next QSO you log with that call** — matched by base
   call, so `/P` suffixes don't break it — is automatically tagged with
   `SIG`/`SIG_INFO` (POTA) or `SOTA_REF` in standard ADIF, ready for the POTA
   uploader.

The pending hunt tags **only the first matching QSO** and **expires after 4
hours**, so a stale park reference can't contaminate an unrelated contact next
week. Activators also appear as chips on the [Needed board](needed-dx.md) when
they're heard on the air.

## Honest limits

- **The POTA/SOTA section is hunter-only** — Nexus helps you *chase* activators;
  it isn't an activation logger for running your own park/summit.
- **Winter Field Day shows raw counts, not a computed total** — by design.
- Field Day **won't start until class and section are set** — that's a guard, not
  a bug.
- **The Satellites section's log strip doesn't join Field Day yet** — unlike the
  CW and Phone strips it stays on the general log while a session runs. Not a
  design choice; not wired up yet.
- **You can't tell the Field Day log which band a contact was on** — it records
  a band per contact, but always the one the radio is on at the moment you type
  the entry; nothing lets you name a different one. Log as you work, not
  afterwards.

## Related guides

- [Operate — FT8/FT4 digital](operate-digital.md)
- [Needed — DX that's on the air now](needed-dx.md)
- [Logbook & QSL](logbook-qsl.md)
- [Settings reference — Field Day Setup](settings-reference.md#field-day-setup)
