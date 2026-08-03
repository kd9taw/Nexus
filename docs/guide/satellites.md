# Satellites

The Satellites section answers "which bird can I work, and when?" for *your*
grid. It predicts amateur-satellite passes over your location, keeps your
favorites at the top, plots each pass, lists the working frequencies, and — if
you have a rotator — can auto-track a bird across the sky through a pass.

Satellites is an opt-in section. Turn it on in the first-run wizard or in
[Settings ▸ Features](settings-reference.md#features). It needs your grid set in
[Settings ▸ Station](settings-reference.md#station) to compute passes.

<!-- TODO: capture screenshot — the Satellites pass list with a favorite starred and its polar plot -->

## The tour

**The pass list** shows upcoming passes over your QTH, favorites first. Each row
reads like a plain-language prediction — for example *"ISS in 38 min · 62° · NW→SE
· 9 min"*: time until AOS, maximum elevation, the direction it travels, and how
long it's up.

**The polar plot** draws the pass across the sky (horizon to zenith), so you can
see where to point and how high it climbs.

**Frequencies** for each bird are listed so you know where to listen and where to
transmit.

You can also drop a **Satellite Passes** pane into the [Connect](connect.md) grid
for an at-a-glance next-passes list beside the map, and turn on the
**Satellites (amateur)** map layer to watch the birds move in real time.

<!-- TODO: capture screenshot — the polar plot of a pass with the AOS/LOS direction and max elevation -->

## Core workflows

### Star your favorites

Click the **⭐** on a bird to favorite it. Favorites sort to the top of the pass
list. The ISS is the easiest first target — favorite it and it leads the list.

### Set a pass alarm

Arm an alarm on a pass and Nexus reminds you before AOS so you don't miss it.
(For the loud, repeating "they're on the air" style of alarm, see the
[DXpedition wake-me alarm](dxpeditions.md#set-a-wake-me-alarm) — the same alarm
machinery.)

### Tune around the passband — and get back on the bird

Working a linear bird means chasing a station across the transponder, so turn
the VFO and Nexus follows: it takes your new dial as your position in the
passband and moves your uplink to match (mirrored, if the transponder inverts).
Doppler keeps correcting around wherever you put yourself.

Tune *outside* the passband and that stops — you've left the transponder as far
as Nexus can tell, which is the right call, because the alternative is dragging
somebody's uplink to a passband edge because they QSY'd to 20 m. **Lock on**, in
the Doppler readout, puts you back: it re-runs the transponder pick you already
made, so the radio, the band and the mode all come with it, and you land in the
middle of the passband again.

### Auto-track with a rotator

1. Configure your rotator in
   [Settings ▸ Rig / CAT](settings-reference.md#rig--cat) — pick your model and
   its COM port and Nexus runs the control daemon for you. No hardware? Pick the
   **Dummy (testing)** model, or run `rotctld -m 1` and point Nexus at
   `127.0.0.1:4533` to watch it work.
2. **Arm rotor track** on a pass. Nexus slews the rotator to follow the bird
   across the sky through the pass; the compass shows the track, with °T and °M
   side by side, and a STOP control halts it.

The section is read-only until you arm a track — it won't touch your rotator on
its own.

## Honest limits

- Passes are computed for your grid — **set your Maidenhead locator** first or
  the predictions can't run.
- The bird list is **not everything in orbit** — it is the amateur population:
  satellites with an amateur transmitter on record. Around 430 birds are
  listed and around 367 of those carry current orbital elements.
- Rotor auto-track drives an **az/el** rotator through Hamlib `rotctld`
  (elevation is followed through the pass; an azimuth-only rotator is detected
  automatically and driven in azimuth alone); test it with the Dummy model
  before you trust it on real hardware.

### Where does the bird list come from, and what does a bird's status mean?

The list is **derived, not copied**. It starts from the
[SatNOGS database](https://db.satnogs.org) — the community record of which
satellites carry which transmitters — and keeps every satellite with an
amateur transmitter: one SatNOGS labels *Amateur*, or one transmitting in an
ITU amateur-satellite allocation (the band test is what keeps SO-50, whose
transmitters are all filed as "Unknown"). Orbital elements for those birds are
then assembled from three sources, freshest epoch winning: CelesTrak's
`amateur` group, CelesTrak's `satnogs` group, and the SatNOGS element service.
That last one matters — it is the only source for a bird still catalogued
under a placeholder number, and for birds CelesTrak has no elements for at all.

The list is rebuilt every six hours by the project's mirror, so **a bird going
on or offline reaches you within six hours of SatNOGS recording it** — no app
update needed. Each bird's status rides with it:

- **alive** — in orbit, with something amateur transmitting. These are the
  birds that carry elements and appear on the map, in the schedule and in the
  pass list.
- **alive but silent** — in orbit, but the catalog lists no working amateur
  transmitter any more. The pass geometry would still be real; there is
  nothing to work on it.
- **dead** — reported silent.
- **re-entered** — gone. Kept in the list for six months after re-entry, so a
  favorite that stops working has a row that says why, then dropped.
- **pre-launch** — on record but not yet deployed. Nothing to work yet.

Only *alive* birds carry elements, so a bird in any other state shows in the
list with its status and "no elements" rather than a position. **Your ★ stays
put either way** — a bird that dies never vanishes out from under its star,
and search reaches the whole catalog so you can always find it to unstar it.

### How current are the elements?

Every prediction runs on orbital elements (TLEs), and elements decay: a fresh
set predicts a pass to the second, an old one drifts — and pointing and
Doppler drift with it. Nexus keeps the elements current for you and tells you
plainly when it can't:

- **Where they come from.** Elements refresh in the background a few times a
  day from the project's mirror described above (CelesTrak data courtesy of
  Dr. T.S. Kelso; population and status data from SatNOGS / Libre Space
  Foundation, CC BY-SA 4.0); the mirror exists so a fleet of installs never
  hammers the sources. If the mirror itself is unreachable for a day, Nexus
  falls back to fetching CelesTrak's amateur group directly — a shorter list
  of 97, with every other bird keeping its row marked "no elements" until the
  mirror is back. The Satellites section and Settings ▸ Orbital elements show
  the age, fetch time and source. **Update now** forces a refresh; **Import
  from file** loads a downloaded TLE/keps file — the path for offline shacks
  and brand-new launches no source carries yet.
- **Past 14 days** the age is badged stale, and arming a pass asks first —
  refresh right there, or arm anyway with your eyes open.
- **Past 30 days** SGP4 accuracy is genuinely gone, so anything that would
  move the radio or rotator refuses — naming the bird and the age — rather
  than drive the antenna off a fiction.
- **A pass runs on the elements it armed with** (a pass is minutes long; a
  mid-pass swap would jump the antenna). The readiness rail shows the age of
  the frozen set.
- **Renames don't orphan your stars.** CelesTrak occasionally renames a bird;
  your ★s, alarms and schedule keep working because Nexus remembers the
  catalog number behind each name.
- A bad or empty download never replaces a good cache, and nothing waits on
  the network — Nexus serves the best elements it has and refreshes behind
  you.

## Related guides

- [Connect — map + propagation](connect.md) (Satellite Passes pane, live map layer)
- [Settings reference](settings-reference.md) (rotator setup)
- [DXpeditions](dxpeditions.md) (the same alarm machinery)
